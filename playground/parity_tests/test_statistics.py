import statistics
from decimal import Decimal
from fractions import Fraction
import math

# === mean ===
try:
    print('mean_basic', statistics.mean([1, 2, 3, 4, 5]))
    print('mean_single', statistics.mean([42]))
    print('mean_floats', statistics.mean([1.5, 2.5, 3.5]))
    print('mean_negative', statistics.mean([-1, -2, -3, -4, -5]))
    print('mean_mixed', statistics.mean([-1, 0, 1]))
    print('mean_fractions', statistics.mean([Fraction(1, 2), Fraction(3, 4)]))
    print('mean_decimals', statistics.mean([Decimal('0.5'), Decimal('1.5')]))
except Exception as e:
    print('SKIP_mean', type(e).__name__, e)

# === fmean ===
try:
    print('fmean_basic', statistics.fmean([1, 2, 3, 4, 5]))
    print('fmean_single', statistics.fmean([42]))
    print('fmean_weights', statistics.fmean([85, 92, 83, 91], weights=[0.20, 0.20, 0.30, 0.30]))
    print('fmean_equal_weights', statistics.fmean([1, 2, 3], weights=[1, 1, 1]))
    print('fmean_floats', statistics.fmean([1.5, 2.5, 3.5]))
except Exception as e:
    print('SKIP_fmean', type(e).__name__, e)

# === geometric_mean ===
try:
    print('geometric_mean_basic', statistics.geometric_mean([54, 24, 36]))
    print('geometric_mean_two', statistics.geometric_mean([2, 8]))
    print('geometric_mean_single', statistics.geometric_mean([42]))
    print('geometric_mean_floats', statistics.geometric_mean([1.0, 2.0, 4.0]))
except Exception as e:
    print('SKIP_geometric_mean', type(e).__name__, e)

# === harmonic_mean ===
try:
    print('harmonic_mean_basic', statistics.harmonic_mean([40, 60]))
    print('harmonic_mean_weights', statistics.harmonic_mean([40, 60], weights=[5, 30]))
    print('harmonic_mean_single', statistics.harmonic_mean([42]))
    print('harmonic_mean_three', statistics.harmonic_mean([1, 2, 3]))
    print('harmonic_mean_with_zero', statistics.harmonic_mean([0, 40, 60]))
except Exception as e:
    print('SKIP_harmonic_mean', type(e).__name__, e)

# === median ===
try:
    print('median_odd', statistics.median([1, 2, 3, 4, 5]))
    print('median_even', statistics.median([1, 2, 3, 4]))
    print('median_single', statistics.median([42]))
    print('median_two', statistics.median([1, 3]))
    print('median_unsorted', statistics.median([5, 1, 3, 2, 4]))
except Exception as e:
    print('SKIP_median', type(e).__name__, e)

# === median_low ===
try:
    print('median_low_odd', statistics.median_low([1, 2, 3, 4, 5]))
    print('median_low_even', statistics.median_low([1, 2, 3, 4]))
    print('median_low_single', statistics.median_low([42]))
except Exception as e:
    print('SKIP_median_low', type(e).__name__, e)

# === median_high ===
try:
    print('median_high_odd', statistics.median_high([1, 2, 3, 4, 5]))
    print('median_high_even', statistics.median_high([1, 2, 3, 4]))
    print('median_high_single', statistics.median_high([42]))
except Exception as e:
    print('SKIP_median_high', type(e).__name__, e)

# === median_grouped ===
try:
    print('median_grouped_basic', statistics.median_grouped([1, 2, 2, 3, 4]))
    print('median_grouped_interval', statistics.median_grouped([1, 2, 2, 3, 4], interval=2))
    print('median_grouped_single', statistics.median_grouped([42]))
    print('median_grouped_even', statistics.median_grouped([1, 2, 3, 4]))
except Exception as e:
    print('SKIP_median_grouped', type(e).__name__, e)

# === mode ===
try:
    print('mode_single', statistics.mode([1, 1, 2, 3]))
    print('mode_string', statistics.mode(['red', 'blue', 'red', 'green']))
    print('mode_integers', statistics.mode([1, 2, 2, 3, 3, 3, 4]))
except Exception as e:
    print('SKIP_mode', type(e).__name__, e)

# === multimode ===
try:
    print('multimode_single', statistics.multimode([1, 1, 2, 3]))
    print('multimode_multiple', statistics.multimode([1, 1, 2, 2, 3]))
    print('multimode_all_unique', statistics.multimode([1, 2, 3]))
    print('multimode_string', statistics.multimode(['red', 'blue', 'red', 'green', 'blue']))
except Exception as e:
    print('SKIP_multimode', type(e).__name__, e)

# === pstdev ===
try:
    print('pstdev_basic', statistics.pstdev([1.5, 2.5, 2.5, 2.75, 3.25, 4.75]))
    print('pstdev_single', statistics.pstdev([42]))
    print('pstdev_mean', statistics.pstdev([1, 2, 3, 4, 5], mu=3))
except Exception as e:
    print('SKIP_pstdev', type(e).__name__, e)

# === pvariance ===
try:
    print('pvariance_basic', statistics.pvariance([0.0, 0.25, 0.25, 1.25, 1.5, 1.75, 2.0, 3.0]))
    print('pvariance_single', statistics.pvariance([42]))
    print('pvariance_mean', statistics.pvariance([1, 2, 3, 4, 5], mu=3))
    print('pvariance_decimals', statistics.pvariance([Decimal('0.5'), Decimal('1.5'), Decimal('2.5')]))
except Exception as e:
    print('SKIP_pvariance', type(e).__name__, e)

# === stdev ===
try:
    print('stdev_basic', statistics.stdev([1.5, 2.5, 2.5, 2.75, 3.25, 4.75]))
    print('stdev_two', statistics.stdev([1, 2]))
    print('stdev_xbar', statistics.stdev([1, 2, 3, 4, 5], xbar=3))
except Exception as e:
    print('SKIP_stdev', type(e).__name__, e)

# === variance ===
try:
    print('variance_basic', statistics.variance([2.75, 1.75, 1.25, 0.25, 0.5, 1.25, 3.5]))
    print('variance_two', statistics.variance([1, 2]))
    print('variance_xbar', statistics.variance([1, 2, 3, 4, 5], xbar=3))
    print('variance_decimals', statistics.variance([Decimal('0.5'), Decimal('1.5'), Decimal('2.5')]))
except Exception as e:
    print('SKIP_variance', type(e).__name__, e)

# === quantiles ===
try:
    print('quantiles_default', statistics.quantiles([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]))
    print('quantiles_n4', statistics.quantiles([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], n=4))
    print('quantiles_n2', statistics.quantiles([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], n=2))
    print('quantiles_method_exclusive', statistics.quantiles([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], method='exclusive'))
    print('quantiles_method_inclusive', statistics.quantiles([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], method='inclusive'))
except Exception as e:
    print('SKIP_quantiles', type(e).__name__, e)

# === covariance ===
try:
    print('covariance_basic', statistics.covariance([1, 2, 3, 4, 5], [2, 4, 6, 8, 10]))
    print('covariance_negative', statistics.covariance([1, 2, 3, 4, 5], [5, 4, 3, 2, 1]))
    print('covariance_independent', statistics.covariance([1, 2, 3], [4, 5, 6]))
except Exception as e:
    print('SKIP_covariance', type(e).__name__, e)

# === correlation ===
try:
    print('correlation_perfect_positive', statistics.correlation([1, 2, 3, 4, 5], [2, 4, 6, 8, 10]))
    print('correlation_perfect_negative', statistics.correlation([1, 2, 3, 4, 5], [10, 8, 6, 4, 2]))
    print('correlation_no_relation', statistics.correlation([1, 2, 3], [4, 5, 6]))
    print('correlation_basic', statistics.correlation([1, 2, 3, 4, 5], [5, 4, 3, 2, 1]))
except Exception as e:
    print('SKIP_correlation', type(e).__name__, e)

# === linear_regression ===
try:
    print('linear_regression_perfect', statistics.linear_regression([1, 2, 3], [2, 4, 6]))
    print('linear_regression_basic', statistics.linear_regression([1, 2, 3, 4, 5], [2, 3, 5, 4, 6]))
    print('linear_regression_negative', statistics.linear_regression([1, 2, 3, 4, 5], [10, 8, 6, 4, 2]))
except Exception as e:
    print('SKIP_linear_regression', type(e).__name__, e)

# === fsum ===
try:
    print('fsum_basic', statistics.fsum([0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]))
    print('fsum_empty', statistics.fsum([]))
    print('fsum_single', statistics.fsum([42.5]))
    print('fsum_mixed', statistics.fsum([1, 2.5, 3]))
except Exception as e:
    print('SKIP_fsum', type(e).__name__, e)

# === sumprod ===
try:
    print('sumprod_basic', statistics.sumprod([1, 2, 3], [4, 5, 6]))
    print('sumprod_empty', statistics.sumprod([], []))
    print('sumprod_single', statistics.sumprod([2], [3]))
    print('sumprod_negative', statistics.sumprod([1, -2, 3], [4, 5, 6]))
except Exception as e:
    print('SKIP_sumprod', type(e).__name__, e)

# === kde ===
try:
    data_kde = [1, 2, 3, 4, 5]
    kde_func = statistics.kde(data_kde, h=1.0)
    print('kde_normal_0', kde_func(0))
    print('kde_normal_3', kde_func(3))
    print('kde_normal_6', kde_func(6))
    
    # kde with cumulative
    kde_cum = statistics.kde(data_kde, h=1.0, cumulative=True)
    print('kde_cumulative_0', kde_cum(0))
    print('kde_cumulative_3', kde_cum(3))
    print('kde_cumulative_6', kde_cum(6))
except Exception as e:
    print('SKIP_kde', type(e).__name__, e)

# === kde_random ===
try:
    data_kde = [1, 2, 3, 4, 5]
    kde_rand = statistics.kde_random(data_kde, h=1.0)
    # Just verify it returns a callable that produces values
    sample = [kde_rand() for _ in range(5)]
    print('kde_random_sample_length', len(sample))
    print('kde_random_sample_types', all(isinstance(x, float) for x in sample))
except Exception as e:
    print('SKIP_kde_random', type(e).__name__, e)

# === NormalDist class ===
try:
    # __init__
    nd_default = statistics.NormalDist()
    print('normaldist_default_mean', nd_default.mean)
    print('normaldist_default_stdev', nd_default.stdev)
    
    nd_custom = statistics.NormalDist(100, 15)
    print('normaldist_custom_mean', nd_custom.mean)
    print('normaldist_custom_stdev', nd_custom.stdev)
    
    # from_samples
    nd_samples = statistics.NormalDist.from_samples([1, 2, 3, 4, 5])
    print('normaldist_from_samples_mean', nd_samples.mean)
    print('normaldist_from_samples_stdev', nd_samples.stdev)
    
    # samples is a property (tuple of mean, stdev)
    print('normaldist_samples', nd_custom.samples)
    
    # pdf
    print('normaldist_pdf_mean', nd_custom.pdf(100))
    print('normaldist_pdf_away', nd_custom.pdf(115))
    print('normaldist_pdf_far', nd_custom.pdf(130))
    
    # cdf
    print('normaldist_cdf_mean', nd_custom.cdf(100))
    print('normaldist_cdf_one_sd', nd_custom.cdf(115))
    print('normaldist_cdf_two_sd', nd_custom.cdf(130))
    
    # inv_cdf
    print('normaldist_inv_cdf_0_5', nd_custom.inv_cdf(0.5))
    print('normaldist_inv_cdf_0_84', nd_custom.inv_cdf(0.8413447460685429))
    print('normaldist_inv_cdf_0_025', nd_custom.inv_cdf(0.025))
    print('normaldist_inv_cdf_0_975', nd_custom.inv_cdf(0.975))
    
    # overlap
    nd1 = statistics.NormalDist(0, 1)
    nd2 = statistics.NormalDist(1, 1)
    print('normaldist_overlap', nd1.overlap(nd2))
    
    nd3 = statistics.NormalDist(0, 1)
    nd4 = statistics.NormalDist(0, 1)
    print('normaldist_overlap_identical', nd3.overlap(nd4))
    
    # quantiles
    nd_std = statistics.NormalDist(0, 1)
    print('normaldist_quantiles_default', nd_std.quantiles())
    print('normaldist_quantiles_n4', nd_std.quantiles(n=4))
    
    # zscore
    print('normaldist_zscore_mean', nd_custom.zscore(100))
    print('normaldist_zscore_one_sd', nd_custom.zscore(115))
    print('normaldist_zscore_minus_one_sd', nd_custom.zscore(85))
    
    # __add__
    nd_sum = nd_custom + 10
    print('normaldist_add_mean', nd_sum.mean)
    print('normaldist_add_stdev', nd_sum.stdev)
    
    # __sub__
    nd_sub = nd_custom - 10
    print('normaldist_sub_mean', nd_sub.mean)
    print('normaldist_sub_stdev', nd_sub.stdev)
    
    # __mul__
    nd_mul = nd_custom * 2
    print('normaldist_mul_mean', nd_mul.mean)
    print('normaldist_mul_stdev', nd_mul.stdev)
    
    # __truediv__
    nd_div = nd_custom / 2
    print('normaldist_div_mean', nd_div.mean)
    print('normaldist_div_stdev', nd_div.stdev)
    
    # __pos__
    nd_pos = +nd_custom
    print('normaldist_pos_mean', nd_pos.mean)
    print('normaldist_pos_stdev', nd_pos.stdev)
    
    # __neg__
    nd_neg = -nd_custom
    print('normaldist_neg_mean', nd_neg.mean)
    print('normaldist_neg_stdev', nd_neg.stdev)
    
    # __radd__
    nd_radd = 10 + nd_custom
    print('normaldist_radd_mean', nd_radd.mean)
    print('normaldist_radd_stdev', nd_radd.stdev)
    
    # __rmul__
    nd_rmul = 2 * nd_custom
    print('normaldist_rmul_mean', nd_rmul.mean)
    print('normaldist_rmul_stdev', nd_rmul.stdev)
    
    # __eq__
    nd_eq1 = statistics.NormalDist(100, 15)
    nd_eq2 = statistics.NormalDist(100, 15)
    nd_eq3 = statistics.NormalDist(100, 20)
    print('normaldist_eq_same', nd_eq1 == nd_eq2)
    print('normaldist_eq_different', nd_eq1 == nd_eq3)
    
    # __repr__
    print('normaldist_repr', repr(nd_custom))
except Exception as e:
    print('SKIP_NormalDist_class', type(e).__name__, e)

# === LinearRegression class ===
try:
    lr = statistics.linear_regression([1, 2, 3], [2, 4, 6])
    print('linearregression_slope', lr.slope)
    print('linearregression_intercept', lr.intercept)
    print('linearregression_repr', repr(lr))
except Exception as e:
    print('SKIP_LinearRegression_class', type(e).__name__, e)

# === StatisticsError ===
try:
    try:
        statistics.mean([])
    except statistics.StatisticsError as e:
        print('statisticserror_mean_empty', str(e))
    
    try:
        statistics.median([])
    except statistics.StatisticsError as e:
        print('statisticserror_median_empty', str(e))
    
    try:
        statistics.mode([])
    except statistics.StatisticsError as e:
        print('statisticserror_mode_empty', str(e))
    
    try:
        statistics.geometric_mean([])
    except statistics.StatisticsError as e:
        print('statisticserror_geometric_mean_empty', str(e))
    
    try:
        statistics.harmonic_mean([])
    except statistics.StatisticsError as e:
        print('statisticserror_harmonic_mean_empty', str(e))
    
    try:
        statistics.pstdev([])
    except statistics.StatisticsError as e:
        print('statisticserror_pstdev_empty', str(e))
    
    try:
        statistics.pvariance([])
    except statistics.StatisticsError as e:
        print('statisticserror_pvariance_empty', str(e))
    
    try:
        statistics.stdev([1])
    except statistics.StatisticsError as e:
        print('statisticserror_stdev_single', str(e))
    
    try:
        statistics.variance([1])
    except statistics.StatisticsError as e:
        print('statisticserror_variance_single', str(e))
    
    try:
        statistics.quantiles([])
    except statistics.StatisticsError as e:
        print('statisticserror_quantiles_empty', str(e))
    
    try:
        statistics.covariance([1, 2], [3])
    except statistics.StatisticsError as e:
        print('statisticserror_covariance_length', str(e))
    
    try:
        statistics.correlation([1, 2], [3])
    except statistics.StatisticsError as e:
        print('statisticserror_correlation_length', str(e))
    
    try:
        statistics.linear_regression([1, 2], [3])
    except statistics.StatisticsError as e:
        print('statisticserror_linear_regression_length', str(e))
    
    try:
        statistics.NormalDist().inv_cdf(-0.1)
    except ValueError as e:
        print('valuederror_inv_cdf_negative', str(e))
    
    try:
        statistics.NormalDist().inv_cdf(1.1)
    except ValueError as e:
        print('valuederror_inv_cdf_over_one', str(e))
    
    # Edge cases with special values
    print('mean_with_zeros', statistics.mean([0, 0, 0]))
    print('mean_large_numbers', statistics.mean([10**10, 10**10 + 1, 10**10 + 2]))
    print('mean_small_numbers', statistics.mean([1e-10, 2e-10, 3e-10]))
    
    # Test with iterators/generators
    print('mean_generator', statistics.mean(x for x in [1, 2, 3, 4, 5]))
    print('fmean_generator', statistics.fmean(x for x in [1, 2, 3, 4, 5]))
    print('median_generator', statistics.median(x for x in [1, 2, 3, 4, 5]))
    
    # Test with tuples
    print('mean_tuple', statistics.mean((1, 2, 3, 4, 5)))
    print('fmean_tuple', statistics.fmean((1, 2, 3, 4, 5)))
    print('median_tuple', statistics.median((1, 2, 3, 4, 5)))
except Exception as e:
    print('SKIP_StatisticsError', type(e).__name__, e)
