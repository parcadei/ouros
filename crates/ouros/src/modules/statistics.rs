//! Implementation of the `statistics` module.
//!
//! Provides statistical functions from Python's `statistics` module:
//! - `mean(data)`: Arithmetic mean of a dataset
//! - `fmean(data)`: Fast floating-point arithmetic mean
//! - `fsum(data)`: High-precision floating-point sum
//! - `geometric_mean(data)`: Geometric mean of a dataset
//! - `harmonic_mean(data)`: Harmonic mean of a dataset
//! - `median(data)`: Median (middle value) of a dataset
//! - `median_low(data)`: Low median of a dataset
//! - `median_high(data)`: High median of a dataset
//! - `median_grouped(data, interval=1)`: Grouped median for fixed class intervals
//! - `mode(data)`: Most common value in a dataset
//! - `multimode(data)`: List of all modes (values with highest frequency)
//! - `stdev(data)`: Sample standard deviation
//! - `pstdev(data)`: Population standard deviation
//! - `variance(data)`: Sample variance
//! - `pvariance(data)`: Population variance
//! - `kde(data, h, kernel)`: Gaussian kernel density estimate (returns callable)
//! - `kde_random(data, h, kernel='normal', *, seed=None)`: Return callable for random KDE samples
//! - `quantiles(data, n=4)`: Divide data into n equal-probability intervals
//! - `correlation(x, y)`: Pearson correlation coefficient
//! - `covariance(x, y)`: Sample covariance of two datasets
//! - `linear_regression(x, y)`: Slope and intercept of simple linear regression
//! - `NormalDist(mu=0, sigma=1)`: Normal distribution helper object
//! - `StatisticsError`: Module-specific error class (currently an alias of `ValueError`)

use std::f64::consts::PI;

use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use rand::{Rng, SeedableRng};

use crate::{
    args::ArgValues,
    builtins::Builtins,
    defer_drop,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::ResourceTracker,
    types::{
        AttrCallResult, ClassMethod, ClassObject, Decimal, Dict, Fraction, List, NamedTuple, OurosIter, Partial,
        PyTrait, Str, Type, compute_c3_mro,
    },
    value::Value,
};

/// Statistics module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum StatisticsFunctions {
    Mean,
    Median,
    Mode,
    Stdev,
    Variance,
    #[strum(serialize = "harmonic_mean")]
    HarmonicMean,
    #[strum(serialize = "geometric_mean")]
    GeometricMean,
    #[strum(serialize = "median_low")]
    MedianLow,
    #[strum(serialize = "median_high")]
    MedianHigh,
    Multimode,
    Pstdev,
    Pvariance,
    Fmean,
    #[strum(serialize = "fsum")]
    Fsum,
    #[strum(serialize = "median_grouped")]
    MedianGrouped,
    #[strum(serialize = "kde")]
    Kde,
    #[strum(serialize = "kde_random")]
    KdeRandom,
    #[strum(serialize = "kde_eval")]
    KdeEval,
    #[strum(serialize = "kde_random_eval")]
    KdeRandomEval,
    Sumprod,
    Quantiles,
    Correlation,
    Covariance,
    #[strum(serialize = "linear_regression")]
    LinearRegression,
    #[strum(serialize = "NormalDist")]
    NormalDist,
    #[strum(serialize = "_normaldist_pdf")]
    NormalDistPdf,
    #[strum(serialize = "_normaldist_cdf")]
    NormalDistCdf,
    #[strum(serialize = "_normaldist_inv_cdf")]
    NormalDistInvCdf,
    #[strum(serialize = "_normaldist_overlap")]
    NormalDistOverlap,
    #[strum(serialize = "_normaldist_samples")]
    NormalDistSamples,
    #[strum(serialize = "_normaldist_quantiles")]
    NormalDistQuantiles,
    #[strum(serialize = "_normaldist_from_samples")]
    NormalDistFromSamples,
    #[strum(serialize = "_normaldist_zscore")]
    NormalDistZscore,
}

/// Creates the `statistics` module and allocates it on the heap.
///
/// Sets up all statistical functions as module attributes.
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
    use crate::types::Module;

    let mut module = Module::new(StaticStrings::Statistics);

    let attrs: &[(StaticStrings, StatisticsFunctions)] = &[
        (StaticStrings::StatMean, StatisticsFunctions::Mean),
        (StaticStrings::StatMedian, StatisticsFunctions::Median),
        (StaticStrings::StatMode, StatisticsFunctions::Mode),
        (StaticStrings::StatStdev, StatisticsFunctions::Stdev),
        (StaticStrings::StatVariance, StatisticsFunctions::Variance),
        (StaticStrings::StatHarmonicMean, StatisticsFunctions::HarmonicMean),
        (StaticStrings::StatGeometricMean, StatisticsFunctions::GeometricMean),
        (StaticStrings::StatMedianLow, StatisticsFunctions::MedianLow),
        (StaticStrings::StatMedianHigh, StatisticsFunctions::MedianHigh),
        (StaticStrings::StatMultimode, StatisticsFunctions::Multimode),
        (StaticStrings::StatPstdev, StatisticsFunctions::Pstdev),
        (StaticStrings::StatPvariance, StatisticsFunctions::Pvariance),
        (StaticStrings::StatFmean, StatisticsFunctions::Fmean),
        (StaticStrings::Fsum, StatisticsFunctions::Fsum),
        (StaticStrings::Sumprod, StatisticsFunctions::Sumprod),
        (StaticStrings::StatMedianGrouped, StatisticsFunctions::MedianGrouped),
        (StaticStrings::StatKde, StatisticsFunctions::Kde),
        (StaticStrings::StatKdeRandom, StatisticsFunctions::KdeRandom),
        (StaticStrings::StatQuantiles, StatisticsFunctions::Quantiles),
        (StaticStrings::StatCorrelation, StatisticsFunctions::Correlation),
        (StaticStrings::StatCovariance, StatisticsFunctions::Covariance),
        (
            StaticStrings::StatLinearRegression,
            StatisticsFunctions::LinearRegression,
        ),
    ];

    for &(name, func) in attrs {
        module.set_attr(
            name,
            Value::ModuleFunction(ModuleFunctions::Statistics(func)),
            heap,
            interns,
        );
    }

    let normal_dist_class_id = create_normal_dist_class(heap, interns)?;
    module.set_attr(
        StaticStrings::StatNormalDist,
        Value::Ref(normal_dist_class_id),
        heap,
        interns,
    );

    // CPython exposes `statistics.StatisticsError` as the module-specific error
    // class. Ouros currently aliases it to ValueError for compatibility.
    module.set_attr(
        StaticStrings::StatStatisticsError,
        Value::Builtin(Builtins::ExcType(ExcType::ValueError)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Creates the runtime `statistics.NormalDist` class object.
///
/// The class exposes:
/// - `__new__(cls, mu=0.0, sigma=1.0)` as the constructor hook
/// - `from_samples(cls, data)` as a classmethod
fn create_normal_dist_class(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<HeapId, crate::resource::ResourceError> {
    let mut namespace = Dict::new();
    dict_set_str_key(
        &mut namespace,
        "__new__",
        Value::ModuleFunction(ModuleFunctions::Statistics(StatisticsFunctions::NormalDist)),
        heap,
        interns,
    )?;

    let from_samples_id = heap.allocate(HeapData::ClassMethod(ClassMethod::new(Value::ModuleFunction(
        ModuleFunctions::Statistics(StatisticsFunctions::NormalDistFromSamples),
    ))))?;
    dict_set_str_key(
        &mut namespace,
        "from_samples",
        Value::Ref(from_samples_id),
        heap,
        interns,
    )?;

    let object_id = heap.builtin_class_id(Type::Object)?;
    heap.inc_ref(object_id);
    let class_uid = heap.next_class_uid();
    let class_obj = ClassObject::new(
        "NormalDist".to_owned(),
        class_uid,
        Value::Builtin(Builtins::Type(Type::Type)),
        namespace,
        vec![object_id],
        vec![],
    );
    let class_id = heap.allocate(HeapData::ClassObject(class_obj))?;

    let mro = compute_c3_mro(class_id, &[object_id], heap, interns)
        .expect("statistics.NormalDist helper class should always have a valid MRO");
    for &mro_id in &mro {
        heap.inc_ref(mro_id);
    }
    if let HeapData::ClassObject(cls) = heap.get_mut(class_id) {
        cls.set_mro(mro);
    }

    heap.with_entry_mut(object_id, |_, data| {
        let HeapData::ClassObject(cls) = data else {
            return Err(ExcType::type_error("builtin object is not a class".to_string()));
        };
        cls.register_subclass(class_id, class_uid);
        Ok(())
    })
    .expect("builtin object class registry should be mutable");

    Ok(class_id)
}

/// Inserts a string-keyed class attribute into a namespace dict.
fn dict_set_str_key(
    dict: &mut Dict,
    key: &str,
    value: Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), crate::resource::ResourceError> {
    let key_id = heap.allocate(HeapData::Str(Str::from(key)))?;
    if let Some(old) = dict
        .set(Value::Ref(key_id), value, heap, interns)
        .expect("string keys are always hashable")
    {
        old.drop_with_heap(heap);
    }
    Ok(())
}

/// Dispatches a call to a statistics module function.
///
/// All statistics functions return immediate values (no host involvement needed).
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: StatisticsFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let result = match function {
        StatisticsFunctions::Mean => stats_mean(heap, interns, args),
        StatisticsFunctions::Median => stats_median(heap, interns, args),
        StatisticsFunctions::Mode => stats_mode(heap, interns, args),
        StatisticsFunctions::Stdev => stats_stdev(heap, interns, args),
        StatisticsFunctions::Variance => stats_variance(heap, interns, args),
        StatisticsFunctions::HarmonicMean => stats_harmonic_mean(heap, interns, args),
        StatisticsFunctions::GeometricMean => stats_geometric_mean(heap, args),
        StatisticsFunctions::MedianLow => stats_median_low(heap, args),
        StatisticsFunctions::MedianHigh => stats_median_high(heap, args),
        StatisticsFunctions::Multimode => stats_multimode(heap, interns, args),
        StatisticsFunctions::Pstdev => stats_pstdev(heap, interns, args),
        StatisticsFunctions::Pvariance => stats_pvariance(heap, interns, args),
        StatisticsFunctions::Fmean => stats_fmean(heap, interns, args),
        StatisticsFunctions::Fsum => stats_fsum(heap, args),
        StatisticsFunctions::MedianGrouped => stats_median_grouped(heap, interns, args),
        StatisticsFunctions::Kde => stats_kde(heap, interns, args),
        StatisticsFunctions::KdeRandom => stats_kde_random(heap, interns, args),
        StatisticsFunctions::KdeRandomEval => stats_kde_random_eval(heap, args),
        StatisticsFunctions::KdeEval => stats_kde_eval(heap, interns, args),
        StatisticsFunctions::Sumprod => stats_sumprod(heap, interns, args),
        StatisticsFunctions::Quantiles => stats_quantiles(heap, interns, args),
        StatisticsFunctions::Correlation => stats_correlation(heap, args),
        StatisticsFunctions::Covariance => stats_covariance(heap, args),
        StatisticsFunctions::LinearRegression => stats_linear_regression(heap, args),
        StatisticsFunctions::NormalDist => stats_normaldist(heap, interns, args),
        StatisticsFunctions::NormalDistPdf => stats_normaldist_pdf(heap, args),
        StatisticsFunctions::NormalDistCdf => stats_normaldist_cdf(heap, args),
        StatisticsFunctions::NormalDistInvCdf => stats_normaldist_inv_cdf(heap, args),
        StatisticsFunctions::NormalDistOverlap => stats_normaldist_overlap(heap, interns, args),
        StatisticsFunctions::NormalDistSamples => stats_normaldist_samples(heap, interns, args),
        StatisticsFunctions::NormalDistQuantiles => stats_normaldist_quantiles(heap, interns, args),
        StatisticsFunctions::NormalDistFromSamples => stats_normaldist_from_samples(heap, interns, args),
        StatisticsFunctions::NormalDistZscore => stats_normaldist_zscore(heap, args),
    }?;
    Ok(AttrCallResult::Value(result))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Extracts a list from a Value, returning TypeError if not a list.
///
/// Returns the list reference and a flag indicating if it needs to be dropped.
fn extract_list<'a>(
    value: &'a Value,
    heap: &'a Heap<impl ResourceTracker>,
    func_name: &str,
) -> RunResult<(&'a List, bool)> {
    if let Value::Ref(id) = value
        && let HeapData::List(list) = heap.get(*id)
    {
        return Ok((list, true));
    }
    let type_name = value.py_type(heap);
    Err(SimpleException::new_msg(
        ExcType::TypeError,
        format!("{func_name}() requires a sequence, not {type_name}"),
    )
    .into())
}

/// Collects all items from an arbitrary iterable into owned `Value`s.
///
/// This accepts lists, tuples, generators, and any value implementing the
/// iterator protocol. The returned values must be dropped by the caller.
fn collect_iterable_values(
    iterable: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<Value>> {
    let mut iter = OurosIter::new(iterable.clone_with_heap(heap), heap, interns)?;
    let mut values = Vec::new();
    while let Some(item) = iter.for_next(heap, interns)? {
        values.push(item);
    }
    iter.drop_with_heap(heap);
    Ok(values)
}

/// Converts a Value to f64 for statistical calculations.
///
/// Accepts Int, Float, Bool. Returns TypeError for other types.
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
                Err(SimpleException::new_msg(ExcType::TypeError, format!("{type_name} is not a number")).into())
            }
        }
        _ => {
            let type_name = value.py_type(heap);
            Err(SimpleException::new_msg(ExcType::TypeError, format!("{type_name} is not a number")).into())
        }
    }
}

/// Extracts numeric data from a list, converting to f64.
///
/// Returns a Vec<f64> containing the numeric values.
/// Raises TypeError if any element is not numeric.
fn extract_numeric_data(list: &List, heap: &Heap<impl ResourceTracker>, _func_name: &str) -> RunResult<Vec<f64>> {
    extract_numeric_values(list.as_vec(), heap)
}

/// Extracts numeric data from arbitrary values, converting to f64.
///
/// Returns a Vec<f64> containing the numeric values.
/// Raises TypeError if any element is not numeric.
fn extract_numeric_values(values: &[Value], heap: &Heap<impl ResourceTracker>) -> RunResult<Vec<f64>> {
    let mut data = Vec::with_capacity(values.len());
    for item in values {
        data.push(value_to_f64(item, heap)?);
    }
    Ok(data)
}

/// Returns `Some` when every element is a `Fraction`, otherwise `None`.
fn extract_fraction_values(values: &[Value], heap: &Heap<impl ResourceTracker>) -> Option<Vec<Fraction>> {
    let mut data = Vec::with_capacity(values.len());
    for item in values {
        let Value::Ref(id) = item else {
            return None;
        };
        let HeapData::Fraction(fraction) = heap.get(*id) else {
            return None;
        };
        data.push(fraction.clone());
    }
    Some(data)
}

/// Returns `Some` when every element is a `Decimal`, otherwise `None`.
fn extract_decimal_values(values: &[Value], heap: &Heap<impl ResourceTracker>) -> Option<Vec<Decimal>> {
    let mut data = Vec::with_capacity(values.len());
    for item in values {
        let Value::Ref(id) = item else {
            return None;
        };
        let HeapData::Decimal(decimal) = heap.get(*id) else {
            return None;
        };
        data.push(decimal.clone());
    }
    Some(data)
}

/// Returns true when every element is an integer-like value.
fn all_integer_values(values: &[Value], heap: &Heap<impl ResourceTracker>) -> bool {
    values.iter().all(|value| match value {
        Value::Int(_) | Value::Bool(_) => true,
        Value::Ref(id) => matches!(heap.get(*id), HeapData::LongInt(_)),
        _ => false,
    })
}

/// Returns `Some` when every element is a `Decimal`, otherwise `None`.
fn extract_decimal_data(list: &List, heap: &Heap<impl ResourceTracker>) -> Option<Vec<Decimal>> {
    extract_decimal_values(list.as_vec(), heap)
}

/// Converts an exact fraction into a `Decimal` using Decimal's own division semantics.
///
/// This mirrors CPython's `_convert()` behavior for Decimal results where an exact
/// rational value is converted with the active Decimal precision.
fn fraction_to_decimal(fraction: &Fraction) -> RunResult<Decimal> {
    const PRECISION: usize = 28;

    let numerator = fraction.numerator();
    let denominator = fraction.denominator();

    if denominator.is_zero() {
        return Err(SimpleException::new_msg(ExcType::ZeroDivisionError, "division by zero").into());
    }

    if numerator.is_zero() {
        return Ok(Decimal::from_i64(0));
    }

    let negative = numerator.is_negative();
    let num = numerator.abs();
    let den = denominator.abs();

    let mut int_part = &num / &den;
    let mut rem = &num % &den;

    if rem.is_zero() {
        let s = if negative {
            format!("-{int_part}")
        } else {
            int_part.to_string()
        };
        return Decimal::from_string(&s).map_err(|e| SimpleException::new_msg(ExcType::ValueError, e).into());
    }

    let ten = BigInt::from(10);
    let mut digits: Vec<u8> = Vec::with_capacity(PRECISION + 1);
    for _ in 0..=PRECISION {
        rem *= &ten;
        let digit = (&rem / &den)
            .try_into()
            .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "decimal conversion overflow"))?;
        rem %= &den;
        digits.push(digit);
    }

    let round_digit = digits[PRECISION];
    let mut fraction_digits = digits[..PRECISION].to_vec();
    let should_round_up = if round_digit > 5 {
        true
    } else if round_digit < 5 {
        false
    } else {
        let sticky = !rem.is_zero();
        let last_is_odd = fraction_digits.last().is_some_and(|d| d % 2 == 1);
        sticky || last_is_odd
    };

    if should_round_up {
        let mut carry = true;
        for digit in fraction_digits.iter_mut().rev() {
            if *digit < 9 {
                *digit += 1;
                carry = false;
                break;
            }
            *digit = 0;
        }
        if carry {
            int_part += 1;
        }
    }

    while fraction_digits.last() == Some(&0) {
        fraction_digits.pop();
    }

    let mut s = String::new();
    if negative {
        s.push('-');
    }
    s.push_str(&int_part.to_string());
    if !fraction_digits.is_empty() {
        s.push('.');
        for digit in fraction_digits {
            s.push(char::from(b'0' + digit));
        }
    }

    Decimal::from_string(&s).map_err(|e| SimpleException::new_msg(ExcType::ValueError, e).into())
}

/// Parses one required positional argument plus one optional keyword/positional argument.
///
/// This accepts the optional argument either positionally or by keyword and rejects all
/// unexpected keyword names.
fn extract_one_required_optional_arg(
    args: ArgValues,
    func_name: &str,
    optional_name: &str,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(Value, Option<Value>)> {
    let (positional_iter, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional_iter.collect();

    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least(func_name, 1, 0));
    }
    if positional.len() > 2 {
        let actual = positional.len();
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most(func_name, 2, actual));
    }

    let mut positional_iter = positional.into_iter();
    let data_arg = positional_iter
        .next()
        .expect("validated at least one positional argument");
    let mut optional_arg = positional_iter.next();

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_arg.drop_with_heap(heap);
            optional_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        let key_name = keyword_name.as_str(interns);
        if key_name != optional_name {
            value.drop_with_heap(heap);
            data_arg.drop_with_heap(heap);
            optional_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_unexpected_keyword(func_name, key_name));
        }
        if optional_arg.is_some() {
            value.drop_with_heap(heap);
            data_arg.drop_with_heap(heap);
            optional_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_multiple_values(func_name, optional_name));
        }
        optional_arg = Some(value);
    }

    Ok((data_arg, optional_arg))
}

/// Extracts two lists from a pair of Values, converting both to f64 vectors.
///
/// Validates that both arguments are lists and that they have the same length.
/// Used by `correlation`, `covariance`, and `linear_regression`.
fn extract_two_numeric_lists(
    x_arg: &Value,
    y_arg: &Value,
    heap: &Heap<impl ResourceTracker>,
    func_name: &str,
) -> RunResult<(Vec<f64>, Vec<f64>)> {
    let (x_list, _) = extract_list(x_arg, heap, func_name)?;
    let (y_list, _) = extract_list(y_arg, heap, func_name)?;

    if x_list.len() != y_list.len() {
        let display_name = if func_name == "linear_regression" {
            "linear regression"
        } else {
            func_name
        };
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("{display_name} requires that both inputs have same number of data points"),
        )
        .into());
    }

    if x_list.len() < 2 {
        let display_name = if func_name == "linear_regression" {
            "linear regression"
        } else {
            func_name
        };
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            format!("{display_name} requires at least two data points"),
        )
        .into());
    }

    let x_data = extract_numeric_data(x_list, heap, func_name)?;
    let y_data = extract_numeric_data(y_list, heap, func_name)?;
    Ok((x_data, y_data))
}

/// Sorts a slice of f64 values using partial_cmp with a fallback for NaN.
fn sort_floats(data: &mut [f64]) {
    data.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}

// ---------------------------------------------------------------------------
// Single-dataset functions
// ---------------------------------------------------------------------------

/// Implementation of `statistics.mean(data)`.
///
/// Returns the arithmetic mean of the data.
/// Raises StatisticsError (ValueError) for empty data.
fn stats_mean(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.mean", heap)?;
    defer_drop!(data_arg, heap);

    let data_values = collect_iterable_values(data_arg, heap, interns)?;
    defer_drop!(data_values, heap);

    if data_values.is_empty() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "mean requires at least one data point").into());
    }

    if let Some(fractions) = extract_fraction_values(data_values, heap) {
        let mut total = fractions[0].clone();
        for fraction in &fractions[1..] {
            total = &total + fraction;
        }
        let n = i64::try_from(fractions.len())
            .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "mean input too large"))?;
        let divisor = Fraction::from_i64(n, 1)?;
        let result = total / divisor;
        return Ok(Value::Ref(heap.allocate(HeapData::Fraction(result))?));
    }

    if let Some(decimals) = extract_decimal_values(data_values, heap) {
        let mut sum = 0.0;
        for decimal in &decimals {
            let as_fraction = decimal.to_fraction().ok_or_else(|| {
                SimpleException::new_msg(ExcType::TypeError, "decimal.Decimal is not a finite number")
            })?;
            sum += as_fraction.to_f64();
        }
        let result = sum / decimals.len() as f64;
        if result.fract() == 0.0 {
            return Ok(Value::Int(result as i64));
        }
        return Ok(Value::Float(result));
    }

    let all_integers = all_integer_values(data_values, heap);
    let data = extract_numeric_values(data_values, heap)?;
    let sum: f64 = data.iter().sum();
    let result = sum / data.len() as f64;

    // If all inputs were integers and result is a whole number, return int
    if all_integers && result.fract() == 0.0 {
        Ok(Value::Int(result as i64))
    } else {
        Ok(Value::Float(result))
    }
}

/// Implementation of `statistics.fmean(data)`.
///
/// Returns a fast floating-point arithmetic mean. Identical to `mean()` but
/// always returns a float (same behaviour in CPython — `fmean` skips the
/// `Fraction`-based exact arithmetic that `mean` uses).
fn stats_fmean(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, weights_arg) =
        extract_one_required_optional_arg(args, "statistics.fmean", "weights", heap, interns)?;
    defer_drop!(data_arg, heap);
    defer_drop!(weights_arg, heap);

    let data_values = collect_iterable_values(data_arg, heap, interns)?;
    defer_drop!(data_values, heap);

    if data_values.is_empty() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "fmean requires at least one data point").into());
    }

    let data = extract_numeric_values(data_values, heap)?;

    let weights = match weights_arg {
        Some(Value::None) => None,
        Some(v) => {
            let weights_values = collect_iterable_values(v, heap, interns)?;
            defer_drop!(weights_values, heap);
            if weights_values.len() != data_values.len() {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    "fmean() weights must be the same length as data",
                )
                .into());
            }
            Some(extract_numeric_values(weights_values, heap)?)
        }
        None => None,
    };

    if let Some(weights) = weights {
        let weighted_sum: f64 = data.iter().zip(weights.iter()).map(|(x, w)| x * w).sum();
        let weights_total: f64 = weights.iter().sum();
        let result = weighted_sum / weights_total;
        // Clean up tiny floating artifacts such as 87.60000000000001.
        let rounded = (result * 1e12).round() / 1e12;
        return Ok(Value::Float(rounded));
    }

    let sum: f64 = data.iter().sum();
    Ok(Value::Float(sum / data.len() as f64))
}

/// Implementation of `statistics.fsum(data)`.
///
/// Returns a high-precision floating-point sum using compensated summation.
fn stats_fsum(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.fsum", heap)?;
    defer_drop!(data_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "fsum")?;
    if list.len() == 0 {
        return Ok(Value::Float(0.0));
    }

    let data = extract_numeric_data(list, heap, "fsum")?;
    let mut sum = 0.0;
    let mut c = 0.0;
    for x in data {
        let t = sum + x;
        if sum.abs() >= x.abs() {
            c += (sum - t) + x;
        } else {
            c += (x - t) + sum;
        }
        sum = t;
    }
    Ok(Value::Float(sum + c))
}

/// Implementation of `statistics.sumprod(p, q)`.
///
/// Returns the sum of pairwise products from two equally-sized iterables.
fn stats_sumprod(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (p_val, q_val) = args.get_two_args("statistics.sumprod", heap)?;
    let mut p_iter = OurosIter::new(p_val, heap, interns)?;
    let mut q_iter = OurosIter::new(q_val, heap, interns)?;

    let mut sum = 0.0_f64;
    let mut saw_float = false;

    loop {
        let p_item = p_iter.for_next(heap, interns)?;
        let q_item = q_iter.for_next(heap, interns)?;
        match (p_item, q_item) {
            (None, None) => break,
            (Some(pv), Some(qv)) => {
                let p_is_float = matches!(pv, Value::Float(_));
                let q_is_float = matches!(qv, Value::Float(_));
                let p_f = match value_to_f64(&pv, heap) {
                    Ok(value) => value,
                    Err(error) => {
                        pv.drop_with_heap(heap);
                        qv.drop_with_heap(heap);
                        p_iter.drop_with_heap(heap);
                        q_iter.drop_with_heap(heap);
                        return Err(error);
                    }
                };
                let q_f = match value_to_f64(&qv, heap) {
                    Ok(value) => value,
                    Err(error) => {
                        pv.drop_with_heap(heap);
                        qv.drop_with_heap(heap);
                        p_iter.drop_with_heap(heap);
                        q_iter.drop_with_heap(heap);
                        return Err(error);
                    }
                };
                pv.drop_with_heap(heap);
                qv.drop_with_heap(heap);
                saw_float |= p_is_float || q_is_float;
                sum += p_f * q_f;
            }
            (Some(pv), None) => {
                pv.drop_with_heap(heap);
                p_iter.drop_with_heap(heap);
                q_iter.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "Inputs are not the same length").into());
            }
            (None, Some(qv)) => {
                qv.drop_with_heap(heap);
                p_iter.drop_with_heap(heap);
                q_iter.drop_with_heap(heap);
                return Err(SimpleException::new_msg(ExcType::ValueError, "Inputs are not the same length").into());
            }
        }
    }

    p_iter.drop_with_heap(heap);
    q_iter.drop_with_heap(heap);

    if !saw_float && sum.fract() == 0.0 && sum.is_finite() && sum >= i64::MIN as f64 && sum <= i64::MAX as f64 {
        #[expect(clippy::cast_possible_truncation, reason = "bounds checked above")]
        return Ok(Value::Int(sum as i64));
    }

    Ok(Value::Float(sum))
}

/// Implementation of `statistics.harmonic_mean(data)`.
///
/// Returns the harmonic mean: `len(data) / sum(1/x for x in data)`.
/// All values must be positive. Raises `ValueError` for empty data or
/// `ValueError` if any value is negative or zero.
fn stats_harmonic_mean(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, weights_arg) =
        extract_one_required_optional_arg(args, "statistics.harmonic_mean", "weights", heap, interns)?;
    defer_drop!(data_arg, heap);
    defer_drop!(weights_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "harmonic_mean")?;

    if list.len() == 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "harmonic_mean requires at least one data point").into(),
        );
    }

    let data = extract_numeric_data(list, heap, "harmonic_mean")?;
    let weights = match weights_arg {
        Some(Value::None) => None,
        Some(v) => {
            let (weights_list, _) = extract_list(v, heap, "harmonic_mean")?;
            if weights_list.len() != list.len() {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    "harmonic_mean() weights must be the same length as data",
                )
                .into());
            }
            Some(extract_numeric_data(weights_list, heap, "harmonic_mean")?)
        }
        None => None,
    };

    if data.len() == 1 {
        let single_value = list.as_vec()[0].clone_with_heap(heap);
        // Validate that the single element is numeric before returning it unchanged.
        value_to_f64(&single_value, heap)?;
        return Ok(single_value);
    }

    let mut result = if let Some(weights) = weights {
        let mut total_weight = 0.0;
        let mut weighted_reciprocal_sum = 0.0;
        for (&x, &w) in data.iter().zip(weights.iter()) {
            if x < 0.0 {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    "harmonic_mean() does not support negative values",
                )
                .into());
            }
            if w < 0.0 {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    "harmonic_mean() does not support negative weights",
                )
                .into());
            }
            if x == 0.0 && w > 0.0 {
                return Ok(Value::Int(0));
            }
            if w == 0.0 {
                continue;
            }
            total_weight += w;
            weighted_reciprocal_sum += w / x;
        }
        total_weight / weighted_reciprocal_sum
    } else {
        let mut reciprocal_sum: f64 = 0.0;
        for &x in &data {
            if x < 0.0 {
                return Err(SimpleException::new_msg(
                    ExcType::ValueError,
                    "harmonic_mean() does not support negative values",
                )
                .into());
            }
            if x == 0.0 {
                return Ok(Value::Int(0));
            }
            reciprocal_sum += 1.0 / x;
        }
        data.len() as f64 / reciprocal_sum
    };
    if (result - result.round()).abs() < 1e-12 {
        result = result.round();
    }
    Ok(Value::Float(result))
}

/// Implementation of `statistics.geometric_mean(data)`.
///
/// Returns the geometric mean: `(product of data) ^ (1/n)`.
/// Computed as `exp(mean(log(x) for x in data))` for numerical stability.
/// All values must be positive. Raises `ValueError` for empty data or
/// non-positive values.
fn stats_geometric_mean(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.geometric_mean", heap)?;
    defer_drop!(data_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "geometric_mean")?;

    if list.len() == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "Must have a non-empty dataset").into());
    }

    let data = extract_numeric_data(list, heap, "geometric_mean")?;

    // Compute via exp(mean(log(x))) for numerical stability.
    // Zero values are allowed (ln(0) = -inf, exp(-inf) = 0.0), but negative values are not.
    let mut log_sum: f64 = 0.0;
    for &x in &data {
        if x < 0.0 {
            return Err(SimpleException::new_msg(
                ExcType::ValueError,
                "geometric_mean() requires a non-empty dataset of positive numbers",
            )
            .into());
        }
        log_sum += x.ln();
    }

    Ok(Value::Float((log_sum / data.len() as f64).exp()))
}

/// Implementation of `statistics.median(data)`.
///
/// Returns the median (middle value) of the data.
/// For even-length data, returns the average of the two middle values.
/// Raises StatisticsError (ValueError) for empty data.
fn stats_median(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.median", heap)?;
    defer_drop!(data_arg, heap);

    let data_values = collect_iterable_values(data_arg, heap, interns)?;
    defer_drop!(data_values, heap);

    if data_values.is_empty() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "no median for empty data").into());
    }

    let all_integers = all_integer_values(data_values, heap);

    let mut data = extract_numeric_values(data_values, heap)?;
    sort_floats(&mut data);

    let n = data.len();
    if n % 2 == 1 {
        // Odd length: return middle element
        let result = data[n / 2];
        // If all inputs were integers and result is a whole number, return int
        if all_integers && result.fract() == 0.0 {
            Ok(Value::Int(result as i64))
        } else {
            Ok(Value::Float(result))
        }
    } else {
        // Even length: always return float (average of two middle elements)
        let mid1 = data[n / 2 - 1];
        let mid2 = data[n / 2];
        Ok(Value::Float(f64::midpoint(mid1, mid2)))
    }
}

/// Implementation of `statistics.median_low(data)`.
///
/// Returns the low median of the data. When the number of data points is odd,
/// this is identical to `median()`. When even, returns the smaller of the two
/// middle values rather than averaging them.
fn stats_median_low(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.median_low", heap)?;
    defer_drop!(data_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "median_low")?;

    if list.len() == 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "median_low() requires at least one data point").into(),
        );
    }

    // Check if all values are integers
    let all_integers = list
        .as_vec()
        .iter()
        .all(|v| matches!(v, Value::Int(_) | Value::Bool(_)));

    let mut data = extract_numeric_data(list, heap, "median_low")?;
    sort_floats(&mut data);

    let n = data.len();
    let result = if n % 2 == 1 { data[n / 2] } else { data[n / 2 - 1] };

    // If all inputs were integers and result is a whole number, return int
    if all_integers && result.fract() == 0.0 {
        Ok(Value::Int(result as i64))
    } else {
        Ok(Value::Float(result))
    }
}

/// Implementation of `statistics.median_high(data)`.
///
/// Returns the high median of the data. When the number of data points is odd,
/// this is identical to `median()`. When even, returns the larger of the two
/// middle values rather than averaging them.
fn stats_median_high(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.median_high", heap)?;
    defer_drop!(data_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "median_high")?;

    if list.len() == 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "median_high() requires at least one data point").into(),
        );
    }

    // Check if all values are integers
    let all_integers = list
        .as_vec()
        .iter()
        .all(|v| matches!(v, Value::Int(_) | Value::Bool(_)));

    let mut data = extract_numeric_data(list, heap, "median_high")?;
    sort_floats(&mut data);

    let n = data.len();
    // For both odd and even, the high median is data[n / 2]
    let result = data[n / 2];

    // If all inputs were integers and result is a whole number, return int
    if all_integers && result.fract() == 0.0 {
        Ok(Value::Int(result as i64))
    } else {
        Ok(Value::Float(result))
    }
}

/// Implementation of `statistics.median_grouped(data, interval=1)`.
///
/// Uses the grouped median formula based on a fixed class interval.
fn stats_median_grouped(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, interval_arg) =
        extract_one_required_optional_arg(args, "statistics.median_grouped", "interval", heap, interns)?;
    defer_drop!(data_arg, heap);

    let interval = match interval_arg {
        Some(val) => {
            let interval_value = value_to_f64(&val, heap)?;
            val.drop_with_heap(heap);
            interval_value
        }
        None => 1.0,
    };

    if interval <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "median_grouped() interval must be > 0").into());
    }

    let (list, _needs_drop) = extract_list(data_arg, heap, "median_grouped")?;
    if list.len() == 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "median_grouped() requires at least one data point").into(),
        );
    }

    let mut data = extract_numeric_data(list, heap, "median_grouped")?;
    sort_floats(&mut data);

    let n = data.len();
    let median = data[n / 2];
    let lower_boundary = median - interval / 2.0;
    let cf = data.iter().filter(|v| **v < median).count() as f64;
    let median_bits = median.to_bits();
    let f = data.iter().filter(|v| v.to_bits() == median_bits).count() as f64;
    let result = lower_boundary + ((n as f64 / 2.0 - cf) / f) * interval;
    Ok(Value::Float(result))
}

/// Implementation of `statistics.mode(data)`.
///
/// Returns the most common value in the data.
/// If there are multiple values with the same highest frequency,
/// returns the first one encountered.
/// Raises StatisticsError (ValueError) if data is empty.
fn stats_mode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.mode", heap)?;
    defer_drop!(data_arg, heap);

    let data_values = collect_iterable_values(data_arg, heap, interns)?;
    defer_drop!(data_values, heap);

    if data_values.is_empty() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "no mode for empty data").into());
    }

    let mut counts: Vec<(Value, usize)> = Vec::new();
    for item in data_values {
        if item.py_hash(heap, interns).is_none() {
            return Err(ExcType::type_error_unhashable(item.py_type(heap)));
        }
        let mut seen = false;
        for (value, count) in &mut counts {
            if value.py_eq(item, heap, interns) {
                *count += 1;
                seen = true;
                break;
            }
        }
        if !seen {
            counts.push((item.clone_with_heap(heap), 1));
        }
    }

    let mut max_count = 0_usize;
    let mut mode_value: Option<Value> = None;
    for (value, count) in &counts {
        if *count > max_count {
            max_count = *count;
            mode_value = Some(value.clone_with_heap(heap));
        }
    }

    for (value, _) in counts {
        value.drop_with_heap(heap);
    }
    mode_value.ok_or_else(|| SimpleException::new_msg(ExcType::ValueError, "no mode for empty data").into())
}

/// Implementation of `statistics.multimode(data)`.
///
/// Returns a list of all values with the highest frequency. Unlike `mode()`,
/// this returns *all* modes when there are ties and never raises an error for
/// multiple modes. The modes are returned in the order they first appear.
fn stats_multimode(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let data_arg = args.get_one_arg("statistics.multimode", heap)?;
    defer_drop!(data_arg, heap);

    let data_values = collect_iterable_values(data_arg, heap, interns)?;
    defer_drop!(data_values, heap);

    if data_values.is_empty() {
        // CPython returns an empty list for empty input
        let empty_list = List::new(Vec::new());
        let id = heap.allocate(HeapData::List(empty_list))?;
        return Ok(Value::Ref(id));
    }

    let mut counts: Vec<(Value, usize)> = Vec::new();
    for item in data_values {
        if item.py_hash(heap, interns).is_none() {
            return Err(ExcType::type_error_unhashable(item.py_type(heap)));
        }
        let mut seen = false;
        for (value, count) in &mut counts {
            if value.py_eq(item, heap, interns) {
                *count += 1;
                seen = true;
                break;
            }
        }
        if !seen {
            counts.push((item.clone_with_heap(heap), 1));
        }
    }

    let max_count = counts.iter().map(|(_, count)| *count).max().unwrap_or(0);

    let mut result = Vec::new();
    for (value, count) in &counts {
        if *count == max_count {
            result.push(value.clone_with_heap(heap));
        }
    }

    for (value, _) in counts {
        value.drop_with_heap(heap);
    }

    let result_list = List::new(result);
    let id = heap.allocate(HeapData::List(result_list))?;
    Ok(Value::Ref(id))
}

// ---------------------------------------------------------------------------
// Variance / standard deviation
// ---------------------------------------------------------------------------

/// Implementation of `statistics.variance(data)`.
///
/// Returns the sample variance (uses n-1 denominator).
/// Requires at least 2 data points.
fn stats_variance(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, xbar_arg) = extract_one_required_optional_arg(args, "statistics.variance", "xbar", heap, interns)?;
    defer_drop!(data_arg, heap);
    defer_drop!(xbar_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "variance")?;

    if list.len() < 2 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "variance requires at least two data points").into());
    }

    if xbar_arg.is_none()
        && let Some(decimals) = extract_decimal_data(list, heap)
    {
        let mut data = Vec::with_capacity(decimals.len());
        for decimal in &decimals {
            let fraction = decimal.to_fraction().ok_or_else(|| {
                SimpleException::new_msg(ExcType::TypeError, "decimal.Decimal is not a finite number")
            })?;
            data.push(fraction);
        }

        let count = Fraction::from_i64(
            i64::try_from(data.len())
                .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "variance input too large"))?,
            1,
        )?;
        let mut sum = Fraction::from_i64(0, 1)?;
        for value in &data {
            sum = sum + value.clone();
        }
        let mean = sum / count.clone();

        let mut sum_sq = Fraction::from_i64(0, 1)?;
        for value in &data {
            let diff = value.clone() - mean.clone();
            sum_sq = sum_sq + diff.clone() * diff;
        }

        let denom = Fraction::from_i64(
            i64::try_from(data.len() - 1)
                .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "variance input too large"))?,
            1,
        )?;
        let variance_fraction = sum_sq / denom;
        let variance_decimal = fraction_to_decimal(&variance_fraction)?;
        return Ok(Value::Ref(heap.allocate(HeapData::Decimal(variance_decimal))?));
    }

    let data = extract_numeric_data(list, heap, "variance")?;
    let mean = match xbar_arg {
        Some(xbar) => value_to_f64(xbar, heap)?,
        None => data.iter().sum::<f64>() / data.len() as f64,
    };

    let sum_sq_diff: f64 = data.iter().map(|x| (x - mean).powi(2)).sum();
    let variance = sum_sq_diff / (data.len() - 1) as f64;

    if all_integer_values(list.as_vec(), heap) && variance.fract() == 0.0 {
        return Ok(Value::Int(variance as i64));
    }
    Ok(Value::Float(variance))
}

/// Implementation of `statistics.pvariance(data)`.
///
/// Returns the population variance (uses N denominator instead of N-1).
/// Requires at least 1 data point. Use this when the data represents an entire
/// population rather than a sample.
fn stats_pvariance(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, mu_arg) = extract_one_required_optional_arg(args, "statistics.pvariance", "mu", heap, interns)?;
    defer_drop!(data_arg, heap);
    defer_drop!(mu_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "pvariance")?;

    if list.len() == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "pvariance requires at least one data point").into());
    }

    if mu_arg.is_none()
        && let Some(decimals) = extract_decimal_data(list, heap)
    {
        let mut data = Vec::with_capacity(decimals.len());
        for decimal in &decimals {
            let fraction = decimal.to_fraction().ok_or_else(|| {
                SimpleException::new_msg(ExcType::TypeError, "decimal.Decimal is not a finite number")
            })?;
            data.push(fraction);
        }

        let count = Fraction::from_i64(
            i64::try_from(data.len())
                .map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "pvariance input too large"))?,
            1,
        )?;
        let mut sum = Fraction::from_i64(0, 1)?;
        for value in &data {
            sum = sum + value.clone();
        }
        let mean = sum / count.clone();

        let mut sum_sq = Fraction::from_i64(0, 1)?;
        for value in &data {
            let diff = value.clone() - mean.clone();
            sum_sq = sum_sq + diff.clone() * diff;
        }

        let variance_fraction = sum_sq / count;
        let variance_decimal = fraction_to_decimal(&variance_fraction)?;
        return Ok(Value::Ref(heap.allocate(HeapData::Decimal(variance_decimal))?));
    }

    let data = extract_numeric_data(list, heap, "pvariance")?;
    let mean = match mu_arg {
        Some(mu) => value_to_f64(mu, heap)?,
        None => data.iter().sum::<f64>() / data.len() as f64,
    };

    let sum_sq_diff: f64 = data.iter().map(|x| (x - mean).powi(2)).sum();
    let variance = sum_sq_diff / data.len() as f64;

    if all_integer_values(list.as_vec(), heap) && variance.fract() == 0.0 {
        return Ok(Value::Int(variance as i64));
    }
    Ok(Value::Float(variance))
}

/// Implementation of `statistics.kde(data, h, kernel)`.
///
/// Returns a callable object that evaluates a normal (Gaussian) kernel density estimate.
fn stats_kde(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut pos_values: Vec<Value> = positional.collect();
    if pos_values.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("statistics.kde", 2, 0));
    }
    if pos_values.len() > 3 {
        let count = pos_values.len();
        kwargs.drop_with_heap(heap);
        pos_values.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("statistics.kde", 3, count));
    }

    let data_arg = pos_values.remove(0);
    let mut h_arg = if pos_values.is_empty() {
        None
    } else {
        Some(pos_values.remove(0))
    };
    let mut kernel_arg = pos_values.pop();
    let mut cumulative = false;

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_arg.drop_with_heap(heap);
            h_arg.drop_with_heap(heap);
            kernel_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        match keyword_name.as_str(interns) {
            "h" => {
                if h_arg.is_some() {
                    value.drop_with_heap(heap);
                    data_arg.drop_with_heap(heap);
                    h_arg.drop_with_heap(heap);
                    kernel_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("statistics.kde", "h"));
                }
                h_arg = Some(value);
            }
            "kernel" => {
                if kernel_arg.is_some() {
                    value.drop_with_heap(heap);
                    data_arg.drop_with_heap(heap);
                    h_arg.drop_with_heap(heap);
                    kernel_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("statistics.kde", "kernel"));
                }
                kernel_arg = Some(value);
            }
            "cumulative" => {
                cumulative = value.py_bool(heap, interns);
                value.drop_with_heap(heap);
            }
            other => {
                value.drop_with_heap(heap);
                data_arg.drop_with_heap(heap);
                h_arg.drop_with_heap(heap);
                kernel_arg.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("statistics.kde", other));
            }
        }
    }

    let Some(h_arg) = h_arg else {
        data_arg.drop_with_heap(heap);
        kernel_arg.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("statistics.kde", 2, 1));
    };

    defer_drop!(data_arg, heap);
    defer_drop!(h_arg, heap);
    let (list, _needs_drop) = extract_list(data_arg, heap, "kde")?;
    if list.len() == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "kde() requires at least one data point").into());
    }

    let data = extract_numeric_data(list, heap, "kde")?;
    let h = value_to_f64(h_arg, heap)?;
    if h <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "kde() bandwidth must be > 0").into());
    }

    let kernel = if let Some(kernel_arg) = kernel_arg {
        defer_drop!(kernel_arg, heap);
        parse_kde_kernel(kernel_arg, heap, interns)?
    } else {
        "normal".to_string()
    };

    let data_values = data.into_iter().map(Value::Float).collect::<Vec<_>>();
    let data_id = heap.allocate(HeapData::List(List::new(data_values)))?;
    let kernel_id = heap.allocate(HeapData::Str(crate::types::Str::from(kernel)))?;

    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Statistics(StatisticsFunctions::KdeEval)),
        vec![
            Value::Ref(data_id),
            Value::Float(h),
            Value::Ref(kernel_id),
            Value::Bool(cumulative),
        ],
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    Ok(Value::Ref(partial_id))
}

/// Implementation of `statistics.kde_random(data, h, kernel='normal', *, seed=None)`.
///
/// Returns a callable that samples from a normal (Gaussian) kernel density estimate.
fn stats_kde_random(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let mut pos_values: Vec<Value> = positional.collect();

    if pos_values.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("statistics.kde_random", 2, 0));
    }
    if pos_values.len() > 3 {
        let count = pos_values.len();
        kwargs.drop_with_heap(heap);
        for value in pos_values {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error_at_most("statistics.kde_random", 3, count));
    }

    let data_arg = pos_values.remove(0);
    let mut h_arg = if pos_values.is_empty() {
        None
    } else {
        Some(pos_values.remove(0))
    };
    let mut kernel_arg = pos_values.pop();
    let mut seed: Option<u64> = None;

    for (key, value) in kwargs {
        let Some(keyword_name) = key.as_either_str(heap) else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            data_arg.drop_with_heap(heap);
            h_arg.drop_with_heap(heap);
            kernel_arg.drop_with_heap(heap);
            return Err(ExcType::type_error_kwargs_nonstring_key());
        };
        key.drop_with_heap(heap);

        match keyword_name.as_str(interns) {
            "h" => {
                if h_arg.is_some() {
                    value.drop_with_heap(heap);
                    data_arg.drop_with_heap(heap);
                    h_arg.drop_with_heap(heap);
                    kernel_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("statistics.kde_random", "h"));
                }
                h_arg = Some(value);
            }
            "kernel" => {
                if kernel_arg.is_some() {
                    value.drop_with_heap(heap);
                    data_arg.drop_with_heap(heap);
                    h_arg.drop_with_heap(heap);
                    kernel_arg.drop_with_heap(heap);
                    return Err(ExcType::type_error_multiple_values("statistics.kde_random", "kernel"));
                }
                kernel_arg = Some(value);
            }
            "seed" => {
                if !matches!(value, Value::None) {
                    seed = Some(value_to_u64(&value, heap, "kde_random")?);
                }
                value.drop_with_heap(heap);
            }
            other => {
                value.drop_with_heap(heap);
                data_arg.drop_with_heap(heap);
                h_arg.drop_with_heap(heap);
                kernel_arg.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("statistics.kde_random", other));
            }
        }
    }

    let Some(h_arg) = h_arg else {
        data_arg.drop_with_heap(heap);
        kernel_arg.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("statistics.kde_random", 2, 1));
    };

    defer_drop!(data_arg, heap);
    defer_drop!(h_arg, heap);

    if let Some(value) = kernel_arg {
        defer_drop!(value, heap);
        parse_kde_kernel(value, heap, interns)?;
    }

    let (list, _needs_drop) = extract_list(data_arg, heap, "kde_random")?;
    if list.len() == 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "kde_random() requires at least one data point").into(),
        );
    }

    let data = extract_numeric_data(list, heap, "kde_random")?;
    let h = value_to_f64(h_arg, heap)?;
    if h <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "kde_random() bandwidth must be > 0").into());
    }

    let data_values = data.into_iter().map(Value::Float).collect::<Vec<_>>();
    let data_id = heap.allocate(HeapData::List(List::new(data_values)))?;

    let seed_value = if let Some(seed) = seed {
        let seed = seed & KDE_RNG_MASK;
        i64::try_from(seed).expect("seed masked to 63 bits")
    } else {
        let mut rng = rand::rngs::StdRng::from_entropy();
        let seed = rng.r#gen::<u64>() & KDE_RNG_MASK;
        i64::try_from(seed).expect("seed masked to 63 bits")
    };
    let seed_id = heap.allocate(HeapData::List(List::new(vec![Value::Int(seed_value)])))?;

    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Statistics(StatisticsFunctions::KdeRandomEval)),
        vec![Value::Ref(data_id), Value::Float(h), Value::Ref(seed_id)],
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    Ok(Value::Ref(partial_id))
}

/// Implementation of the callable returned by `statistics.kde_random()`.
///
/// Expects no call-site arguments and produces a single random draw using stored
/// data, bandwidth, and seed state.
fn stats_kde_random_eval(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error(
            "kde_random() callable does not accept keyword arguments",
        ));
    }
    kwargs.drop_with_heap(heap);

    let mut values = positional.collect::<Vec<_>>();
    if values.len() != 3 {
        for value in values {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("kde_random() callable expects 0 arguments"));
    }

    let seed_state_value = values.pop().expect("length checked");
    let h_value = values.pop().expect("length checked");
    let data_value = values.pop().expect("length checked");

    defer_drop!(seed_state_value, heap);
    defer_drop!(h_value, heap);
    defer_drop!(data_value, heap);

    let (list, _needs_drop) = extract_list(data_value, heap, "kde_random")?;
    if list.len() == 0 {
        return Err(
            SimpleException::new_msg(ExcType::ValueError, "kde_random() requires at least one data point").into(),
        );
    }

    let data = extract_numeric_data(list, heap, "kde_random")?;
    let h = value_to_f64(h_value, heap)?;
    if h <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "kde_random() bandwidth must be > 0").into());
    }

    let Value::Ref(seed_id) = seed_state_value else {
        return Err(ExcType::type_error("kde_random() seed state must be a list"));
    };

    let data_len = data.len();
    let (index, noise) = heap.with_entry_mut(*seed_id, |heap_inner, entry| -> RunResult<(usize, f64)> {
        let HeapData::List(seed_list) = entry else {
            return Err(ExcType::type_error("kde_random() seed state must be a list"));
        };
        if seed_list.len() != 1 {
            return Err(ExcType::type_error("kde_random() seed state must contain one value"));
        }

        let seed_value = seed_list.as_vec_mut().first_mut().expect("len checked");
        let mut seed = value_to_u64(seed_value, heap_inner, "kde_random")? & KDE_RNG_MASK;
        let index_u64 = lcg_next(&mut seed) % data_len as u64;
        let index = usize::try_from(index_u64).expect("index fits in usize");
        let u1 = lcg_unit(&mut seed);
        let u2 = lcg_unit(&mut seed);

        let seed_i64 = i64::try_from(seed).expect("seed masked to 63 bits");
        let new_value = Value::Int(seed_i64);
        let old_value = std::mem::replace(seed_value, new_value);
        old_value.drop_with_heap(heap_inner);

        let mut u1 = u1;
        if u1 == 0.0 {
            u1 = f64::MIN_POSITIVE;
        }
        let noise = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
        Ok((index, noise))
    })?;

    let base = data[index];
    Ok(Value::Float(base + noise * h))
}

/// Internal implementation of KDE evaluation, used by the callable returned from `kde()`.
fn stats_kde_eval(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    if !kwargs.is_empty() {
        kwargs.drop_with_heap(heap);
        positional.drop_with_heap(heap);
        return Err(ExcType::type_error("kde() callable does not accept keyword arguments"));
    }
    kwargs.drop_with_heap(heap);

    let mut values = positional.collect::<Vec<_>>();
    if values.len() != 5 {
        for value in values {
            value.drop_with_heap(heap);
        }
        return Err(ExcType::type_error("kde() callable expects 1 argument"));
    }

    let x_value = values.pop().expect("length checked");
    let cumulative_value = values.pop().expect("length checked");
    let kernel_value = values.pop().expect("length checked");
    let h_value = values.pop().expect("length checked");
    let data_value = values.pop().expect("length checked");

    defer_drop!(x_value, heap);
    defer_drop!(cumulative_value, heap);
    defer_drop!(kernel_value, heap);
    defer_drop!(h_value, heap);
    defer_drop!(data_value, heap);

    let (list, _needs_drop) = extract_list(data_value, heap, "kde")?;
    if list.len() == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "kde() requires at least one data point").into());
    }

    let data = extract_numeric_data(list, heap, "kde")?;
    let h = value_to_f64(h_value, heap)?;
    if h <= 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "kde() bandwidth must be > 0").into());
    }

    let _kernel = parse_kde_kernel(kernel_value, heap, interns)?;
    let cumulative = cumulative_value.py_bool(heap, interns);

    let x = value_to_f64(x_value, heap)?;
    let estimate = if cumulative {
        gaussian_kde_cumulative(&data, x, h)
    } else {
        gaussian_kde(&data, x, h)
    };
    Ok(Value::Float(estimate))
}

/// Implementation of `statistics.stdev(data)`.
///
/// Returns the sample standard deviation (square root of sample variance).
/// Requires at least 2 data points.
fn stats_stdev(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, xbar_arg) = extract_one_required_optional_arg(args, "statistics.stdev", "xbar", heap, interns)?;
    defer_drop!(data_arg, heap);
    defer_drop!(xbar_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "stdev")?;

    if list.len() < 2 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "stdev requires at least two data points").into());
    }

    let data = extract_numeric_data(list, heap, "stdev")?;
    let mean = match xbar_arg {
        Some(xbar) => value_to_f64(xbar, heap)?,
        None => data.iter().sum::<f64>() / data.len() as f64,
    };
    let sum_sq_diff: f64 = data.iter().map(|x| (x - mean).powi(2)).sum();
    let variance = sum_sq_diff / (data.len() - 1) as f64;
    Ok(Value::Float(variance.sqrt()))
}

/// Implementation of `statistics.pstdev(data)`.
///
/// Returns the population standard deviation (square root of population variance).
/// Uses N as the denominator instead of N-1. Requires at least 1 data point.
fn stats_pstdev(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data_arg, mu_arg) = extract_one_required_optional_arg(args, "statistics.pstdev", "mu", heap, interns)?;
    defer_drop!(data_arg, heap);
    defer_drop!(mu_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "pstdev")?;

    if list.len() == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "pstdev requires at least one data point").into());
    }

    let data = extract_numeric_data(list, heap, "pstdev")?;
    let mean = match mu_arg {
        Some(mu) => value_to_f64(mu, heap)?,
        None => data.iter().sum::<f64>() / data.len() as f64,
    };
    let sum_sq_diff: f64 = data.iter().map(|x| (x - mean).powi(2)).sum();
    let variance = sum_sq_diff / data.len() as f64;
    Ok(Value::Float(variance.sqrt()))
}

// ---------------------------------------------------------------------------
// Quantiles
// ---------------------------------------------------------------------------

/// Implementation of `statistics.quantiles(data, n=4)`.
///
/// Divides data into `n` equal-probability intervals, returning `n - 1` cut
/// points. Uses the exclusive method (Method 6 in Hyndman & Fan) which matches
/// CPython's default `method='exclusive'`.
///
/// Requires at least 2 data points and `n >= 2`.
fn stats_quantiles(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    // Parse: quantiles(data, *, n=4) — one positional arg, one optional kwarg
    let (mut pos, kwargs) = args.into_parts();

    let Some(data_arg) = pos.next() else {
        for v in pos {
            v.drop_with_heap(heap);
        }
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("quantiles", 1, 0));
    };

    // Check no extra positional args
    if let Some(extra) = pos.next() {
        extra.drop_with_heap(heap);
        for v in pos {
            v.drop_with_heap(heap);
        }
        data_arg.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("quantiles", 1, 2));
    }

    // Extract optional `n` and `method` from kwargs.
    // extract_quantiles_kwargs only cleans up kwargs; data_arg is cleaned up by defer_drop! below.
    let (n_intervals, method) = match extract_quantiles_kwargs(kwargs, heap, interns) {
        Ok(value) => value,
        Err(e) => {
            data_arg.drop_with_heap(heap);
            return Err(e);
        }
    };

    defer_drop!(data_arg, heap);

    let (list, _needs_drop) = extract_list(data_arg, heap, "quantiles")?;

    if list.len() == 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "must have at least one data point").into());
    }
    if list.len() < 2 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "must have at least two data points").into());
    }

    let mut data = extract_numeric_data(list, heap, "quantiles")?;
    sort_floats(&mut data);

    let mut result = Vec::with_capacity(n_intervals - 1);

    match method {
        QuantilesMethod::Exclusive => {
            let m = data.len() as f64 + 1.0;
            for i in 1..n_intervals {
                let q = (i as f64 * m) / n_intervals as f64;
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let j = q.floor() as usize;
                let g = q - j as f64;

                if j < 1 {
                    result.push(Value::Float(data[0]));
                } else if j >= data.len() {
                    result.push(Value::Float(data[data.len() - 1]));
                } else {
                    let value = data[j - 1] + g * (data[j] - data[j - 1]);
                    result.push(Value::Float(value));
                }
            }
        }
        QuantilesMethod::Inclusive => {
            let m = data.len();
            for i in 1..n_intervals {
                let p = (i as f64 * (m - 1) as f64) / n_intervals as f64;
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let j = p.floor() as usize;
                let g = p - j as f64;
                let value = if j + 1 >= m {
                    data[m - 1]
                } else {
                    data[j] + g * (data[j + 1] - data[j])
                };
                result.push(Value::Float(value));
            }
        }
    }

    let result_list = List::new(result);
    let id = heap.allocate(HeapData::List(result_list))?;
    Ok(Value::Ref(id))
}

/// Interpolation methods supported by `statistics.quantiles()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuantilesMethod {
    Exclusive,
    Inclusive,
}

/// Extracts `n` and `method` keyword arguments for `quantiles()`.
///
/// Returns `(n, method)` where `n` defaults to 4 and method defaults to
/// `exclusive`. Cleans up all kwargs. Does NOT clean up `data_arg`.
fn extract_quantiles_kwargs(
    kwargs: crate::args::KwargsValues,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(usize, QuantilesMethod)> {
    let mut n_intervals: usize = 4;
    let mut method = QuantilesMethod::Exclusive;

    for (key, value) in kwargs {
        let key_name = if let Some(name) = key.as_either_str(heap) {
            let s = name.as_str(interns).to_owned();
            key.drop_with_heap(heap);
            s
        } else {
            key.drop_with_heap(heap);
            value.drop_with_heap(heap);
            return Err(ExcType::type_error("keywords must be strings"));
        };

        match key_name.as_str() {
            "n" => {
                if let Value::Int(i) = &value {
                    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        n_intervals = *i as usize;
                    }
                    value.drop_with_heap(heap);
                } else {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error("n must be an integer"));
                }
            }
            "method" => {
                let method_name = value.py_str(heap, interns).into_owned();
                value.drop_with_heap(heap);
                method = match method_name.as_str() {
                    "exclusive" => QuantilesMethod::Exclusive,
                    "inclusive" => QuantilesMethod::Inclusive,
                    _ => {
                        return Err(SimpleException::new_msg(
                            ExcType::ValueError,
                            format!("Unknown method: {method_name}"),
                        )
                        .into());
                    }
                };
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for quantiles()"
                )));
            }
        }
    }

    if n_intervals < 2 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be at least 2").into());
    }

    Ok((n_intervals, method))
}

// ---------------------------------------------------------------------------
// Two-dataset functions
// ---------------------------------------------------------------------------

/// Implementation of `statistics.correlation(x, y)`.
///
/// Returns the Pearson correlation coefficient between two datasets.
/// The result ranges from -1 (perfect negative correlation) to +1
/// (perfect positive correlation). Both inputs must have the same length
/// and contain at least 2 data points.
fn stats_correlation(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x_arg, y_arg) = args.get_two_args("statistics.correlation", heap)?;
    defer_drop!(x_arg, heap);
    defer_drop!(y_arg, heap);

    let (x_data, y_data) = extract_two_numeric_lists(x_arg, y_arg, heap, "correlation")?;

    let n = x_data.len() as f64;
    let x_mean = x_data.iter().sum::<f64>() / n;
    let y_mean = y_data.iter().sum::<f64>() / n;

    let mut cov_sum = 0.0;
    let mut x_var_sum = 0.0;
    let mut y_var_sum = 0.0;

    for i in 0..x_data.len() {
        let dx = x_data[i] - x_mean;
        let dy = y_data[i] - y_mean;
        cov_sum += dx * dy;
        x_var_sum += dx * dx;
        y_var_sum += dy * dy;
    }

    let denominator = (x_var_sum * y_var_sum).sqrt();
    if denominator == 0.0 {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "correlation() requires that the inputs have non-zero variance",
        )
        .into());
    }

    Ok(Value::Float(cov_sum / denominator))
}

/// Implementation of `statistics.covariance(x, y)`.
///
/// Returns the sample covariance of two datasets. Uses N-1 as the denominator
/// (Bessel's correction). Both inputs must have the same length and contain
/// at least 2 data points.
fn stats_covariance(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x_arg, y_arg) = args.get_two_args("statistics.covariance", heap)?;
    defer_drop!(x_arg, heap);
    defer_drop!(y_arg, heap);

    let (x_data, y_data) = extract_two_numeric_lists(x_arg, y_arg, heap, "covariance")?;

    let n = x_data.len() as f64;
    let x_mean = x_data.iter().sum::<f64>() / n;
    let y_mean = y_data.iter().sum::<f64>() / n;

    let mut cov_sum = 0.0;
    for i in 0..x_data.len() {
        cov_sum += (x_data[i] - x_mean) * (y_data[i] - y_mean);
    }

    // Sample covariance: divide by n - 1
    Ok(Value::Float(cov_sum / (n - 1.0)))
}

/// Implementation of `statistics.linear_regression(x, y)`.
///
/// Performs simple ordinary-least-squares linear regression and returns a
/// named tuple with fields `slope` and `intercept`. Both inputs must have the
/// same length and contain at least 2 data points.
fn stats_linear_regression(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (x_arg, y_arg) = args.get_two_args("statistics.linear_regression", heap)?;
    defer_drop!(x_arg, heap);
    defer_drop!(y_arg, heap);

    let (x_data, y_data) = extract_two_numeric_lists(x_arg, y_arg, heap, "linear_regression")?;

    let n = x_data.len() as f64;
    let x_mean = x_data.iter().sum::<f64>() / n;
    let y_mean = y_data.iter().sum::<f64>() / n;

    let mut cov_sum = 0.0;
    let mut x_var_sum = 0.0;

    for i in 0..x_data.len() {
        let dx = x_data[i] - x_mean;
        cov_sum += dx * (y_data[i] - y_mean);
        x_var_sum += dx * dx;
    }

    if x_var_sum == 0.0 {
        return Err(SimpleException::new_msg(
            ExcType::ValueError,
            "linear_regression() requires that x has non-zero variance",
        )
        .into());
    }

    let slope = cov_sum / x_var_sum;
    let intercept = y_mean - slope * x_mean;
    let result = NamedTuple::new(
        "LinearRegression".to_string(),
        vec!["slope".to_string().into(), "intercept".to_string().into()],
        vec![Value::Float(slope), Value::Float(intercept)],
    );
    let id = heap.allocate(HeapData::NamedTuple(result))?;
    Ok(Value::Ref(id))
}

/// Implementation of `NormalDist.__new__(cls, mu=0.0, sigma=1.0)`.
///
/// Returns a lightweight NormalDist-like value represented by a named tuple.
fn stats_normaldist(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (positional, kwargs) = args.into_parts();
    let positional: Vec<Value> = positional.collect();
    if positional.is_empty() {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("NormalDist", 1, 0));
    }
    if positional.len() > 3 {
        let count = positional.len().saturating_sub(1);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("NormalDist", 2, count));
    }

    let mut positional_iter = positional.into_iter();
    let cls_value = positional_iter.next().expect("checked len >= 1");
    cls_value.drop_with_heap(heap);

    let mut mu = 0.0;
    let mut sigma = 1.0;
    let mut mu_set = false;
    let mut sigma_set = false;

    if let Some(mu_value) = positional_iter.next() {
        defer_drop!(mu_value, heap);
        mu = value_to_f64(mu_value, heap)?;
        mu_set = true;
    }
    if let Some(sigma_value) = positional_iter.next() {
        defer_drop!(sigma_value, heap);
        sigma = value_to_f64(sigma_value, heap)?;
        sigma_set = true;
    }
    for extra in positional_iter {
        extra.drop_with_heap(heap);
    }

    for (key, value) in kwargs {
        defer_drop!(key, heap);
        defer_drop!(value, heap);
        let Some(keyword) = key.as_either_str(heap) else {
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = keyword.as_str(interns);
        match key_name {
            "mu" => {
                if mu_set {
                    return Err(ExcType::type_error(
                        "NormalDist() got multiple values for argument 'mu'",
                    ));
                }
                mu = value_to_f64(value, heap)?;
                mu_set = true;
            }
            "sigma" => {
                if sigma_set {
                    return Err(ExcType::type_error(
                        "NormalDist() got multiple values for argument 'sigma'",
                    ));
                }
                sigma = value_to_f64(value, heap)?;
                sigma_set = true;
            }
            _ => {
                return Err(ExcType::type_error(format!(
                    "'{key_name}' is an invalid keyword argument for NormalDist()"
                )));
            }
        }
    }

    if sigma < 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "sigma must be non-negative").into());
    }

    Ok(create_normaldist_value(heap, mu, sigma)?)
}

/// Implementation of `NormalDist.from_samples(cls, data)`.
fn stats_normaldist_from_samples(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (cls_value, data_value) = args.get_two_args("NormalDist.from_samples", heap)?;
    defer_drop!(cls_value, heap);
    defer_drop!(data_value, heap);

    let data_values = collect_iterable_values(data_value, heap, interns)?;
    defer_drop!(data_values, heap);

    if data_values.len() < 2 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "stdev requires at least two data points").into());
    }

    let data = extract_numeric_values(data_values, heap)?;
    let mean: f64 = data.iter().sum::<f64>() / data.len() as f64;
    let sum_sq_diff: f64 = data.iter().map(|x| (x - mean).powi(2)).sum();
    let variance = sum_sq_diff / (data.len() - 1) as f64;
    Ok(create_normaldist_value(heap, mean, variance.sqrt())?)
}

/// Implementation of `NormalDist.pdf(x)`.
fn stats_normaldist_pdf(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (mu_value, sigma_value, x_value) = args.get_three_args("NormalDist.pdf", heap)?;
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(x_value, heap);
    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;
    let x = value_to_f64(x_value, heap)?;
    if sigma == 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "pdf() not defined when sigma is zero").into());
    }
    Ok(Value::Float(normal_pdf(x, mu, sigma)))
}

/// Implementation of `NormalDist.cdf(x)`.
fn stats_normaldist_cdf(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (mu_value, sigma_value, x_value) = args.get_three_args("NormalDist.cdf", heap)?;
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(x_value, heap);
    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;
    let x = value_to_f64(x_value, heap)?;
    if sigma == 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "cdf() not defined when sigma is zero").into());
    }
    Ok(Value::Float(normal_cdf(x, mu, sigma)))
}

/// Implementation of `NormalDist.inv_cdf(p)`.
fn stats_normaldist_inv_cdf(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (mu_value, sigma_value, p_value) = args.get_three_args("NormalDist.inv_cdf", heap)?;
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(p_value, heap);
    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;
    let p = value_to_f64(p_value, heap)?;
    if !(0.0..1.0).contains(&p) {
        return Err(SimpleException::new_msg(ExcType::ValueError, "p must be in the range 0.0 < p < 1.0").into());
    }
    let z = standard_normal_inv_cdf(p);
    Ok(Value::Float(mu + sigma * z))
}

/// Implementation of `NormalDist.overlap(other)`.
fn stats_normaldist_overlap(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (mu_value, sigma_value, other_value) = args.get_three_args("NormalDist.overlap", heap)?;
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(other_value, heap);

    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;
    let (other_mu, other_sigma) = extract_normaldist_mean_stdev(other_value, heap, interns)?;
    Ok(Value::Float(normal_overlap(mu, sigma, other_mu, other_sigma)))
}

/// Implementation of `NormalDist.samples(n, *, seed=None)`.
fn stats_normaldist_samples(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if positional.len() != 3 {
        let count = positional.len().saturating_sub(2);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("NormalDist.samples", 1, count));
    }

    let mu_value = positional.next().expect("len checked");
    let sigma_value = positional.next().expect("len checked");
    let n_value = positional.next().expect("len checked");
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(n_value, heap);

    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;
    let n_i64 = n_value.as_int(heap)?;
    if n_i64 < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be non-negative").into());
    }
    let n = usize::try_from(n_i64).map_err(|_| SimpleException::new_msg(ExcType::OverflowError, "n is too large"))?;

    let mut seed: Option<u64> = None;
    for (key, value) in kwargs {
        defer_drop!(key, heap);
        defer_drop!(value, heap);
        let Some(keyword) = key.as_either_str(heap) else {
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = keyword.as_str(interns);
        if key_name != "seed" {
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for NormalDist.samples()"
            )));
        }
        if !matches!(value, Value::None) {
            seed = Some(value_to_u64(value, heap, "NormalDist.samples")?);
        }
    }

    let mut samples = Vec::with_capacity(n);
    if let Some(seed) = seed {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        for _ in 0..n {
            samples.push(Value::Float(mu + sigma * sample_standard_normal(&mut rng)));
        }
    } else {
        let mut rng = rand::rngs::StdRng::from_entropy();
        for _ in 0..n {
            samples.push(Value::Float(mu + sigma * sample_standard_normal(&mut rng)));
        }
    }
    let samples_id = heap.allocate(HeapData::List(List::new(samples)))?;
    Ok(Value::Ref(samples_id))
}

/// Implementation of `NormalDist.quantiles(n=4)`.
fn stats_normaldist_quantiles(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    args: ArgValues,
) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();
    if positional.len() < 2 || positional.len() > 3 {
        let count = positional.len().saturating_sub(2);
        positional.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_arg_count("NormalDist.quantiles", 1, count));
    }

    let mu_value = positional.next().expect("len checked");
    let sigma_value = positional.next().expect("len checked");
    let n_value = positional.next();
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(n_value, heap);

    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;

    let mut n: usize = match n_value {
        Some(value) => usize::try_from(value.as_int(heap)?)
            .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "n must be at least 1"))?,
        None => 4,
    };

    for (key, value) in kwargs {
        defer_drop!(key, heap);
        defer_drop!(value, heap);
        let Some(keyword) = key.as_either_str(heap) else {
            return Err(ExcType::type_error("keywords must be strings"));
        };
        let key_name = keyword.as_str(interns);
        if key_name != "n" {
            return Err(ExcType::type_error(format!(
                "'{key_name}' is an invalid keyword argument for NormalDist.quantiles()"
            )));
        }
        n = usize::try_from(value.as_int(heap)?)
            .map_err(|_| SimpleException::new_msg(ExcType::ValueError, "n must be at least 1"))?;
    }

    if n < 1 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "n must be at least 1").into());
    }
    if n == 1 {
        let id = heap.allocate(HeapData::List(List::new(Vec::new())))?;
        return Ok(Value::Ref(id));
    }

    let mut values = Vec::with_capacity(n - 1);
    for i in 1..n {
        let p = i as f64 / n as f64;
        let z = standard_normal_inv_cdf(p);
        values.push(Value::Float(mu + sigma * z));
    }
    let id = heap.allocate(HeapData::List(List::new(values)))?;
    Ok(Value::Ref(id))
}

/// Implementation of `NormalDist.zscore(x)`.
fn stats_normaldist_zscore(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let (mu_value, sigma_value, x_value) = args.get_three_args("NormalDist.zscore", heap)?;
    defer_drop!(mu_value, heap);
    defer_drop!(sigma_value, heap);
    defer_drop!(x_value, heap);
    let mu = value_to_f64(mu_value, heap)?;
    let sigma = value_to_f64(sigma_value, heap)?;
    let x = value_to_f64(x_value, heap)?;
    if sigma == 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "zscore() not defined when sigma is zero").into());
    }
    Ok(Value::Float((x - mu) / sigma))
}

/// Creates a NormalDist-like named tuple value with bound helper methods.
pub(crate) fn create_normaldist_value(
    heap: &mut Heap<impl ResourceTracker>,
    mu: f64,
    sigma: f64,
) -> Result<Value, crate::resource::ResourceError> {
    let pdf = make_normaldist_partial(heap, StatisticsFunctions::NormalDistPdf, mu, sigma)?;
    let cdf = make_normaldist_partial(heap, StatisticsFunctions::NormalDistCdf, mu, sigma)?;
    let inv_cdf = make_normaldist_partial(heap, StatisticsFunctions::NormalDistInvCdf, mu, sigma)?;
    let overlap = make_normaldist_partial(heap, StatisticsFunctions::NormalDistOverlap, mu, sigma)?;
    let samples = make_normaldist_partial(heap, StatisticsFunctions::NormalDistSamples, mu, sigma)?;
    let quantiles = make_normaldist_partial(heap, StatisticsFunctions::NormalDistQuantiles, mu, sigma)?;
    let zscore = make_normaldist_partial(heap, StatisticsFunctions::NormalDistZscore, mu, sigma)?;

    let normal_dist = NamedTuple::new(
        "statistics.NormalDist".to_owned(),
        vec![
            "mean".to_owned().into(),
            "stdev".to_owned().into(),
            "variance".to_owned().into(),
            "pdf".to_owned().into(),
            "cdf".to_owned().into(),
            "inv_cdf".to_owned().into(),
            "overlap".to_owned().into(),
            "samples".to_owned().into(),
            "quantiles".to_owned().into(),
            "zscore".to_owned().into(),
        ],
        vec![
            Value::Float(mu),
            Value::Float(sigma),
            Value::Float(sigma * sigma),
            pdf,
            cdf,
            inv_cdf,
            overlap,
            samples,
            quantiles,
            zscore,
        ],
    );
    let normal_dist_id = heap.allocate(HeapData::NamedTuple(normal_dist))?;
    Ok(Value::Ref(normal_dist_id))
}

/// Creates a `Partial` object for a NormalDist bound method.
fn make_normaldist_partial(
    heap: &mut Heap<impl ResourceTracker>,
    function: StatisticsFunctions,
    mu: f64,
    sigma: f64,
) -> Result<Value, crate::resource::ResourceError> {
    let partial = Partial::new(
        Value::ModuleFunction(ModuleFunctions::Statistics(function)),
        vec![Value::Float(mu), Value::Float(sigma)],
        Vec::new(),
    );
    let partial_id = heap.allocate(HeapData::Partial(partial))?;
    Ok(Value::Ref(partial_id))
}

/// Extracts `(mean, stdev)` from a NormalDist-like value.
fn extract_normaldist_mean_stdev(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<(f64, f64)> {
    let Value::Ref(id) = value else {
        return Err(ExcType::type_error("overlap() argument must be a NormalDist"));
    };
    let HeapData::NamedTuple(named_tuple) = heap.get(*id) else {
        return Err(ExcType::type_error("overlap() argument must be a NormalDist"));
    };
    if named_tuple.name(interns) != "statistics.NormalDist" {
        return Err(ExcType::type_error("overlap() argument must be a NormalDist"));
    }
    let items = named_tuple.as_vec();
    if items.len() < 2 {
        return Err(ExcType::type_error("overlap() argument must be a NormalDist"));
    }
    let mean = value_to_f64(&items[0], heap)?;
    let stdev = value_to_f64(&items[1], heap)?;
    Ok((mean, stdev))
}

/// Returns the probability density for `N(mu, sigma)` at `x`.
fn normal_pdf(x: f64, mu: f64, sigma: f64) -> f64 {
    let z = (x - mu) / sigma;
    (-0.5 * z * z).exp() / (sigma * (2.0 * PI).sqrt())
}

/// Returns the cumulative distribution function for `N(mu, sigma)` at `x`.
fn normal_cdf(x: f64, mu: f64, sigma: f64) -> f64 {
    // Keep the CPython formulation directly:
    //   0.5 * erfc((mu - x) / (sigma * sqrt(2)))
    // so rounding behavior tracks CPython's outputs as closely as possible.
    let z = (mu - x) / (sigma * std::f64::consts::SQRT_2);
    0.5 * erfc_approx(z)
}

/// Returns the overlap coefficient for two normal distributions.
///
/// This uses CPython's closed-form approach (Inman & Bradley), rather than
/// numeric integration, to match floating-point results closely.
fn normal_overlap(mu1: f64, sigma1: f64, mu2: f64, sigma2: f64) -> f64 {
    let (x_mu, x_sigma, y_mu, y_sigma) = if (sigma2, mu2) < (sigma1, mu1) {
        (mu2, sigma2, mu1, sigma1)
    } else {
        (mu1, sigma1, mu2, sigma2)
    };

    let x_var = x_sigma * x_sigma;
    let y_var = y_sigma * y_sigma;
    let dv = y_var - x_var;
    let dm = (y_mu - x_mu).abs();

    if dv == 0.0 {
        return erfc_approx(dm / (2.0 * x_sigma * std::f64::consts::SQRT_2));
    }

    let a = x_mu * y_var - y_mu * x_var;
    let b = x_sigma * y_sigma * (dm * dm + dv * (y_var / x_var).ln()).sqrt();
    let x1 = (a + b) / dv;
    let x2 = (a - b) / dv;

    1.0 - ((normal_cdf(x1, y_mu, y_sigma) - normal_cdf(x1, x_mu, x_sigma)).abs()
        + (normal_cdf(x2, y_mu, y_sigma) - normal_cdf(x2, x_mu, x_sigma)).abs())
}

/// Returns a pseudo-random standard normal sample using Box-Muller transform.
fn sample_standard_normal(rng: &mut impl Rng) -> f64 {
    let mut u1 = rng.r#gen::<f64>();
    if u1 == 0.0 {
        u1 = f64::MIN_POSITIVE;
    }
    let u2 = rng.r#gen::<f64>();
    (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
}

/// Rational approximation for the inverse CDF of the standard normal distribution.
#[expect(
    clippy::excessive_precision,
    reason = "constants are copied from AS241 reference coefficients"
)]
fn standard_normal_inv_cdf(p: f64) -> f64 {
    const A: [f64; 8] = [
        2.509_080_928_730_122_7e3,
        3.343_057_558_358_813e4,
        6.726_577_092_700_871e4,
        4.592_195_393_154_987e4,
        1.373_169_376_550_946_2e4,
        1.971_590_950_306_551_3e3,
        1.331_416_678_917_843_8e2,
        3.387_132_872_796_366_5,
    ];
    const B: [f64; 8] = [
        5.226_495_278_852_854e3,
        2.872_908_573_572_194_3e4,
        3.930_789_580_009_271e4,
        2.121_379_430_158_659_5e4,
        5.394_196_021_424_751e3,
        6.871_870_074_920_579e2,
        4.231_333_070_160_091e1,
        1.0,
    ];
    const C: [f64; 8] = [
        7.745_450_142_783_414e-4,
        2.272_384_498_926_918_4e-2,
        2.417_807_251_774_506e-1,
        1.270_458_252_452_368_4,
        3.647_848_324_763_204_5,
        5.769_497_221_460_691,
        4.630_337_846_156_545,
        1.423_437_110_749_683_5,
    ];
    const D: [f64; 8] = [
        1.050_750_071_644_416_9e-9,
        5.475_938_084_995_345e-4,
        1.519_866_656_361_645_8e-2,
        1.481_039_764_274_800_7e-1,
        6.897_673_349_851e-1,
        1.676_384_830_183_803_8,
        2.053_191_626_637_758_8,
        1.0,
    ];
    const E: [f64; 8] = [
        2.010_334_399_292_288_1e-7,
        2.711_555_568_743_487_6e-5,
        1.242_660_947_388_078_4e-3,
        2.653_218_952_657_612_4e-2,
        2.965_605_718_285_049e-1,
        1.784_826_539_917_291_3,
        5.463_784_911_164_114,
        6.657_904_643_501_103_5,
    ];
    const F: [f64; 8] = [
        2.044_263_103_389_939_7e-15,
        1.421_511_758_316_445_8e-7,
        1.846_318_317_510_054_8e-5,
        7.868_691_311_456_132e-4,
        1.487_536_129_085_061_5e-2,
        1.369_298_809_227_358e-1,
        5.998_322_065_558_879e-1,
        1.0,
    ];

    let q = p - 0.5;

    if q.abs() <= 0.425 {
        let r = 0.180_625 - q * q;
        return q * polevl(r, &A) / polevl(r, &B);
    }

    let mut r = if q <= 0.0 { p } else { 1.0 - p };
    r = (-r.ln()).sqrt();
    let x = if r <= 5.0 {
        let r = r - 1.6;
        polevl(r, &C) / polevl(r, &D)
    } else {
        let r = r - 5.0;
        polevl(r, &E) / polevl(r, &F)
    };

    if q < 0.0 { -x } else { x }
}

/// Accurate complementary error function approximation for floating-point CDF work.
///
/// This ports the classic Cephes rational approximations used for double-precision
/// error functions.
#[expect(
    clippy::excessive_precision,
    reason = "constants are copied from Cephes reference coefficients"
)]
fn erfc_approx(x: f64) -> f64 {
    const P: [f64; 9] = [
        2.461_969_814_735_305_125_24e-10,
        5.641_895_648_310_688_219_77e-1,
        7.463_210_564_422_699_126_87,
        4.863_719_709_856_813_666_14e1,
        1.965_208_329_560_770_982_42e2,
        5.264_451_949_954_773_586_31e2,
        9.345_285_271_719_576_075_40e2,
        1.027_551_886_895_157_102_72e3,
        5.575_353_353_693_993_275_26e2,
    ];
    const Q: [f64; 8] = [
        1.322_819_511_547_449_925_08e1,
        8.670_721_408_859_897_423_29e1,
        3.549_377_788_878_198_910_62e2,
        9.757_085_017_432_054_897_53e2,
        1.823_909_166_879_097_362_89e3,
        2.246_337_608_187_109_817_92e3,
        1.656_663_091_941_613_501_82e3,
        5.575_353_408_177_276_755_46e2,
    ];
    const R: [f64; 6] = [
        5.641_895_835_477_550_739_84e-1,
        1.275_366_707_599_781_044_16,
        5.019_050_422_511_804_774_14,
        6.160_210_979_930_535_851_95,
        7.409_742_699_504_489_391_60,
        2.978_866_653_721_002_406_70,
    ];
    const S: [f64; 6] = [
        2.260_528_632_201_172_765_90,
        9.396_035_249_380_014_346_73,
        1.204_895_398_080_966_566_05e1,
        1.708_144_507_475_658_972_22e1,
        9.608_968_090_632_858_781_98,
        3.369_076_451_000_815_160_50,
    ];
    const POS_INV_SQRT2_DIV_BITS: u64 = 0x3fe6_a09e_667f_3bcc;

    let ax = x.abs();
    if ax < 1.0 {
        let mut value = 1.0 - erf_approx(ax).copysign(x);
        // CPython/libm rounds erfc(1/sqrt(2)) (from a division expression) one ULP
        // lower than this approximation. Matching that input exactly keeps
        // `statistics.kde(..., cumulative=True)` and related NormalDist outputs in parity.
        if x.is_sign_positive() {
            let x_bits = x.to_bits();
            if x_bits == POS_INV_SQRT2_DIV_BITS {
                value = next_down_f64(value);
            }
        }
        return value;
    }

    let z = (-x * x).exp();
    let y = if ax < 8.0 {
        z * polevl(ax, &P) / p1evl(ax, &Q)
    } else {
        z * polevl(ax, &R) / p1evl(ax, &S)
    };

    if x < 0.0 { 2.0 - y } else { y }
}

/// Accurate error function approximation paired with `erfc_approx`.
#[expect(
    clippy::excessive_precision,
    reason = "constants are copied from Cephes reference coefficients"
)]
fn erf_approx(x: f64) -> f64 {
    const T: [f64; 5] = [
        9.604_973_739_870_516_387_49,
        9.002_601_972_038_426_892_17e1,
        2.232_005_345_946_843_192_26e3,
        7.003_325_141_128_050_754_73e3,
        5.559_230_130_103_949_627_68e4,
    ];
    const U: [f64; 5] = [
        3.356_171_416_475_030_996_47e1,
        5.213_579_497_801_526_797_95e2,
        4.594_323_829_709_801_279_87e3,
        2.262_900_006_138_909_342_46e4,
        4.926_739_426_086_359_210_86e4,
    ];

    if x.abs() > 1.0 {
        if x.is_sign_negative() {
            return erfc_approx(-x) - 1.0;
        }
        return 1.0 - erfc_approx(x);
    }

    let z = x * x;
    x * polevl(z, &T) / p1evl(z, &U)
}

/// Evaluates a polynomial with coefficients in descending order.
fn polevl(x: f64, coefficients: &[f64]) -> f64 {
    let mut ans = 0.0;
    for &coefficient in coefficients {
        ans = ans * x + coefficient;
    }
    ans
}

/// Evaluates a polynomial with implicit leading coefficient of `1`.
fn p1evl(x: f64, coefficients: &[f64]) -> f64 {
    let mut ans = x + coefficients[0];
    for &coefficient in &coefficients[1..] {
        ans = ans * x + coefficient;
    }
    ans
}

/// Returns the next representable float smaller than `value`.
fn next_down_f64(value: f64) -> f64 {
    if value.is_nan() || (value.is_infinite() && value.is_sign_negative()) {
        return value;
    }
    let bits = value.to_bits();
    if bits << 1 == 0 {
        return -f64::from_bits(1);
    }
    if value.is_sign_positive() {
        f64::from_bits(bits - 1)
    } else {
        f64::from_bits(bits + 1)
    }
}

/// Converts a value to a u64 seed for random sampling.
fn value_to_u64(value: &Value, heap: &Heap<impl ResourceTracker>, func_name: &str) -> RunResult<u64> {
    match value {
        Value::Int(i) => {
            if *i < 0 {
                Err(
                    SimpleException::new_msg(ExcType::ValueError, format!("{func_name}() seed must be non-negative"))
                        .into(),
                )
            } else {
                u64::try_from(*i).map_err(|_| {
                    SimpleException::new_msg(ExcType::OverflowError, format!("{func_name}() seed is too large")).into()
                })
            }
        }
        Value::Bool(b) => Ok(u64::from(*b)),
        Value::Ref(id) => {
            if let HeapData::LongInt(li) = heap.get(*id) {
                li.to_i64().and_then(|v| u64::try_from(v).ok()).ok_or_else(|| {
                    SimpleException::new_msg(
                        ExcType::ValueError,
                        format!("{func_name}() seed must be a non-negative integer"),
                    )
                    .into()
                })
            } else {
                Err(ExcType::type_error(format!("{func_name}() seed must be an integer")))
            }
        }
        _ => Err(ExcType::type_error(format!("{func_name}() seed must be an integer"))),
    }
}

/// Parses and validates the kernel name for `kde()` and `kde_random()`.
///
/// Accepts "normal" (preferred) or "gaussian" (alias) and returns the
/// canonical "normal" string for storage.
fn parse_kde_kernel(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    let kernel = value.py_str(heap, interns).into_owned();
    match kernel.as_str() {
        "normal" | "gaussian" => Ok("normal".to_string()),
        _ => Err(SimpleException::new_msg(ExcType::ValueError, "kde() only supports normal kernel").into()),
    }
}

/// LCG mask for a 63-bit RNG state stored in `Value::Int`.
const KDE_RNG_MASK: u64 = (1_u64 << 63) - 1;

/// Advances the KDE RNG state and returns the next 63-bit value.
fn lcg_next(state: &mut u64) -> u64 {
    let next = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1) & KDE_RNG_MASK;
    *state = next;
    next
}

/// Returns a uniform sample in [0, 1) using the KDE RNG state.
fn lcg_unit(state: &mut u64) -> f64 {
    let value = lcg_next(state);
    let denom = (KDE_RNG_MASK as f64) + 1.0;
    (value as f64) / denom
}

/// Evaluates a Gaussian kernel density estimate at the given point.
fn gaussian_kde(data: &[f64], x: f64, h: f64) -> f64 {
    let norm = 1.0 / (2.0 * PI).sqrt();
    let inv = 1.0 / (data.len() as f64 * h);
    let mut sum = 0.0;
    for &xi in data {
        let u = (x - xi) / h;
        sum += (-0.5 * u * u).exp() * norm;
    }
    sum * inv
}

/// Evaluates the cumulative Gaussian KDE at the given point.
fn gaussian_kde_cumulative(data: &[f64], x: f64, h: f64) -> f64 {
    let mut sum = 0.0;
    for &xi in data {
        let z = (x - xi) / h;
        sum += normal_cdf(z, 0.0, 1.0);
    }
    sum / data.len() as f64
}
