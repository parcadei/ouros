# Datetime module parity tests for Monty
# Run against both CPython 3.14 and Monty to verify parity

from datetime import date, time, datetime, timedelta, timezone, MINYEAR, MAXYEAR, UTC, tzinfo

# === Module constants ===
try:
    print('MINYEAR', MINYEAR)
    print('MAXYEAR', MAXYEAR)
    print('UTC_is_timezone', type(UTC) is timezone)
except Exception as e:
    print('SKIP_Module_constants', type(e).__name__, e)

# === timedelta construction ===
try:
    # Basic construction
    td1 = timedelta(days=1)
    print('td_days_1_days', td1.days)

    td2 = timedelta(seconds=3600)
    print('td_seconds_3600_seconds', td2.seconds)

    td3 = timedelta(microseconds=500000)
    print('td_microseconds_500000_microseconds', td3.microseconds)

    # Combined construction
    td4 = timedelta(days=1, seconds=3661, microseconds=123456)
    print('td_combined_days', td4.days)
    print('td_combined_seconds', td4.seconds)
    print('td_combined_microseconds', td4.microseconds)

    # Zero timedelta
    td_zero = timedelta()
    print('td_zero_days', td_zero.days)
    print('td_zero_seconds', td_zero.seconds)
    print('td_zero_microseconds', td_zero.microseconds)
except Exception as e:
    print('SKIP_timedelta_construction', type(e).__name__, e)

# === timedelta attributes ===
try:
    td = timedelta(days=2, seconds=45, microseconds=999999)
    print('td_attr_days', td.days)
    print('td_attr_seconds', td.seconds)
    print('td_attr_microseconds', td.microseconds)
except Exception as e:
    print('SKIP_timedelta_attributes', type(e).__name__, e)

# === timedelta total_seconds ===
try:
    print('td_1day_total_seconds', timedelta(days=1).total_seconds())
    print('td_1hour_total_seconds', timedelta(hours=1).total_seconds())
    print('td_90minutes_total_seconds', timedelta(minutes=90).total_seconds())
    print('td_zero_total_seconds', timedelta().total_seconds())
    print('td_123ms_total_seconds', timedelta(milliseconds=123).total_seconds())
except Exception as e:
    print('SKIP_timedelta_total_seconds', type(e).__name__, e)

# === timedelta arithmetic - addition ===
try:
    td1 = timedelta(days=1)
    td2 = timedelta(hours=12)
    result = td1 + td2
    print('td_add_days', result.days)
    print('td_add_seconds', result.seconds)
except Exception as e:
    print('SKIP_timedelta_arithmetic_-_addition', type(e).__name__, e)

# === timedelta arithmetic - subtraction ===
try:
    td1 = timedelta(days=2)
    td2 = timedelta(hours=6)
    result = td1 - td2
    print('td_sub_days', result.days)
    print('td_sub_seconds', result.seconds)

    # Negative result
    td1 = timedelta(hours=1)
    td2 = timedelta(hours=3)
    result = td1 - td2
    print('td_sub_negative_days', result.days)
    print('td_sub_negative_seconds', result.seconds)
except Exception as e:
    print('SKIP_timedelta_arithmetic_-_subtraction', type(e).__name__, e)

# === timedelta arithmetic - multiplication ===
try:
    td = timedelta(hours=2)
    result = td * 3
    print('td_mul_int_days', result.days)
    print('td_mul_int_seconds', result.seconds)

    result = td * 0
    print('td_mul_zero_days', result.days)
    print('td_mul_zero_seconds', result.seconds)
except Exception as e:
    print('SKIP_timedelta_arithmetic_-_multiplication', type(e).__name__, e)

# === timedelta arithmetic - division ===
try:
    td = timedelta(days=2)
    result = td / 2
    print('td_div_int_days', result.days)

    td = timedelta(hours=6)
    result = td / 3
    print('td_div_int_seconds', result.seconds)
except Exception as e:
    print('SKIP_timedelta_arithmetic_-_division', type(e).__name__, e)

# === timedelta division by timedelta (Python 3.12+) ===
try:
    td1 = timedelta(days=1)
    td2 = timedelta(hours=12)
    result = td1 / td2
    print('td_div_td', result)

    td1 = timedelta(hours=6)
    td2 = timedelta(hours=2)
    result = td1 / td2
    print('td_div_td_int_result', result)
except Exception as e:
    print('SKIP_timedelta_division_by_timedelta_(Python_3.12+)', type(e).__name__, e)

# === timedelta floor division ===
try:
    td1 = timedelta(days=1)
    td2 = timedelta(hours=12)
    result = td1 // td2
    print('td_floordiv_td', result)
except Exception as e:
    print('SKIP_timedelta_floor_division', type(e).__name__, e)

# === timedelta modulo ===
try:
    td1 = timedelta(hours=25)
    td2 = timedelta(hours=12)
    result = td1 % td2
    print('td_mod_td', result)
except Exception as e:
    print('SKIP_timedelta_modulo', type(e).__name__, e)

# === timedelta comparison ===
try:
    td1 = timedelta(days=1)
    td2 = timedelta(days=1)
    td3 = timedelta(hours=12)
    print('td_eq_true', td1 == td2)
    print('td_eq_false', td1 == td3)
    print('td_ne_true', td1 != td3)
    print('td_ne_false', td1 != td2)
    print('td_lt_true', td3 < td1)
    print('td_lt_false', td1 < td3)
    print('td_gt_true', td1 > td3)
    print('td_gt_false', td3 > td1)
except Exception as e:
    print('SKIP_timedelta_comparison', type(e).__name__, e)

# === date construction ===
try:
    d1 = date(2024, 1, 15)
    print('date_2024_1_15_year', d1.year)
    print('date_2024_1_15_month', d1.month)
    print('date_2024_1_15_day', d1.day)
except Exception as e:
    print('SKIP_date_construction', type(e).__name__, e)

# === date.today ===
try:
    # Note: today() returns the current date, so we just verify it works
    today_result = date.today()
    print('date_today_type', type(today_result) is date)
except Exception as e:
    print('SKIP_date.today', type(e).__name__, e)

# === date.fromtimestamp ===
try:
    import time as time_module
    ts = time_module.time()
    d_from_ts = date.fromtimestamp(ts)
    print('date_fromtimestamp_type', type(d_from_ts) is date)
except Exception as e:
    print('SKIP_date.fromtimestamp', type(e).__name__, e)

# === date.fromordinal ===
try:
    d_from_ord = date.fromordinal(738886)  # 2024-01-15
    print('date_fromordinal_year', d_from_ord.year)
    print('date_fromordinal_month', d_from_ord.month)
    print('date_fromordinal_day', d_from_ord.day)
except Exception as e:
    print('SKIP_date.fromordinal', type(e).__name__, e)

# === date.fromisoformat ===
try:
    d_from_iso = date.fromisoformat('2024-03-15')
    print('date_fromisoformat_year', d_from_iso.year)
    print('date_fromisoformat_month', d_from_iso.month)
    print('date_fromisoformat_day', d_from_iso.day)
except Exception as e:
    print('SKIP_date.fromisoformat', type(e).__name__, e)

# === date attributes ===
try:
    d = date(2023, 6, 30)
    print('date_attr_year', d.year)
    print('date_attr_month', d.month)
    print('date_attr_day', d.day)
except Exception as e:
    print('SKIP_date_attributes', type(e).__name__, e)

# === date arithmetic - add timedelta ===
try:
    d = date(2024, 1, 15)
    td = timedelta(days=10)
    result = d + td
    print('date_add_days_year', result.year)
    print('date_add_days_month', result.month)
    print('date_add_days_day', result.day)

    # Month boundary crossing
    d = date(2024, 1, 25)
    result = d + timedelta(days=10)
    print('date_add_cross_month', result.month)
    print('date_add_cross_day', result.day)
except Exception as e:
    print('SKIP_date_arithmetic_-_add_timedelta', type(e).__name__, e)

# === date arithmetic - subtract timedelta ===
try:
    d = date(2024, 1, 15)
    td = timedelta(days=5)
    result = d - td
    print('date_sub_days_day', result.day)

    # Month boundary crossing (subtract)
    d = date(2024, 3, 5)
    result = d - timedelta(days=10)
    print('date_sub_cross_month', result.month)
    print('date_sub_cross_day', result.day)
except Exception as e:
    print('SKIP_date_arithmetic_-_subtract_timedelta', type(e).__name__, e)

# === date arithmetic - date minus date ===
try:
    d1 = date(2024, 1, 15)
    d2 = date(2024, 1, 10)
    result = d1 - d2
    print('date_diff_days', result.days)

    d1 = date(2024, 2, 1)
    d2 = date(2024, 1, 1)
    result = d1 - d2
    print('date_diff_31_days', result.days)

    # Negative difference
    d1 = date(2024, 1, 10)
    d2 = date(2024, 1, 15)
    result = d1 - d2
    print('date_diff_negative_days', result.days)
except Exception as e:
    print('SKIP_date_arithmetic_-_date_minus_date', type(e).__name__, e)

# === date weekday ===
try:
    # Jan 1, 2024 is Monday (0)
    print('date_2024_1_1_weekday', date(2024, 1, 1).weekday())
    # Jan 7, 2024 is Sunday (6)
    print('date_2024_1_7_weekday', date(2024, 1, 7).weekday())
    # Jan 3, 2024 is Wednesday (2)
    print('date_2024_1_3_weekday', date(2024, 1, 3).weekday())
except Exception as e:
    print('SKIP_date_weekday', type(e).__name__, e)

# === date isoweekday ===
try:
    print('date_2024_1_1_isoweekday', date(2024, 1, 1).isoweekday())
    print('date_2024_1_7_isoweekday', date(2024, 1, 7).isoweekday())
except Exception as e:
    print('SKIP_date_isoweekday', type(e).__name__, e)

# === date isocalendar ===
try:
    date_ica = date(2024, 1, 15).isocalendar()
    print('date_isocalendar_year', date_ica.year)
    print('date_isocalendar_week', date_ica.week)
    print('date_isocalendar_weekday', date_ica.weekday)
except Exception as e:
    print('SKIP_date_isocalendar', type(e).__name__, e)

# === date isoformat ===
try:
    print('date_isoformat_basic', date(2024, 1, 15).isoformat())
    print('date_isoformat_dec', date(2024, 12, 25).isoformat())
    print('date_isoformat_single', date(2024, 1, 5).isoformat())
except Exception as e:
    print('SKIP_date_isoformat', type(e).__name__, e)

# === date ctime ===
try:
    print('date_ctime', date(2024, 1, 15).ctime())
except Exception as e:
    print('SKIP_date_ctime', type(e).__name__, e)

# === date strftime ===
try:
    print('date_strftime', date(2024, 1, 15).strftime('%Y-%m-%d'))
    print('date_strftime_2', date(2024, 3, 15).strftime('%B %d, %Y'))
except Exception as e:
    print('SKIP_date_strftime', type(e).__name__, e)

# === date replace ===
try:
    d = date(2024, 1, 15)
    d_replaced = d.replace(year=2025)
    print('date_replace_year', d_replaced.year)
    d_replaced2 = d.replace(month=6)
    print('date_replace_month', d_replaced2.month)
    d_replaced3 = d.replace(day=20)
    print('date_replace_day', d_replaced3.day)
    d_replaced4 = d.replace(year=2025, month=6, day=20)
    print('date_replace_all', d_replaced4.year, d_replaced4.month, d_replaced4.day)
except Exception as e:
    print('SKIP_date_replace', type(e).__name__, e)

# === date toordinal ===
try:
    d = date(2024, 1, 15)
    print('date_toordinal', d.toordinal())
except Exception as e:
    print('SKIP_date_toordinal', type(e).__name__, e)

# === date timetuple ===
try:
    d = date(2024, 1, 15)
    tt = d.timetuple()
    print('date_timetuple_year', tt.tm_year)
    print('date_timetuple_mon', tt.tm_mon)
    print('date_timetuple_mday', tt.tm_mday)
except Exception as e:
    print('SKIP_date_timetuple', type(e).__name__, e)

# === date min/max ===
try:
    print('date_min_year', date.min.year)
    print('date_max_year', date.max.year)
except Exception as e:
    print('SKIP_date_min/max', type(e).__name__, e)

# === datetime construction ===
try:
    dt = datetime(2024, 3, 15, 14, 30, 45, 500000)
    print('dt_year', dt.year)
    print('dt_month', dt.month)
    print('dt_day', dt.day)
    print('dt_hour', dt.hour)
    print('dt_minute', dt.minute)
    print('dt_second', dt.second)
    print('dt_microsecond', dt.microsecond)
except Exception as e:
    print('SKIP_datetime_construction', type(e).__name__, e)

# === datetime construction - defaults ===
try:
    dt = datetime(2024, 1, 15)
    print('dt_default_hour', dt.hour)
    print('dt_default_minute', dt.minute)
    print('dt_default_second', dt.second)
    print('dt_default_microsecond', dt.microsecond)
except Exception as e:
    print('SKIP_datetime_construction_-_defaults', type(e).__name__, e)

# === datetime.combine ===
try:
    d = date(2024, 3, 15)
    t = time(14, 30, 45)
    dt_combined = datetime.combine(d, t)
    print('dt_combine_year', dt_combined.year)
    print('dt_combine_hour', dt_combined.hour)
except Exception as e:
    print('SKIP_datetime.combine', type(e).__name__, e)

# === datetime.now ===
try:
    dt_now = datetime.now()
    print('dt_now_type', type(dt_now) is datetime)
except Exception as e:
    print('SKIP_datetime.now', type(e).__name__, e)

# === datetime.utcnow ===
try:
    dt_utcnow = datetime.utcnow()
    print('dt_utcnow_type', type(dt_utcnow) is datetime)
except Exception as e:
    print('SKIP_datetime.utcnow', type(e).__name__, e)

# === datetime.fromtimestamp ===
try:
    import time as time_module
    ts = time_module.time()
    dt_from_ts = datetime.fromtimestamp(ts)
    print('dt_fromtimestamp_type', type(dt_from_ts) is datetime)
except Exception as e:
    print('SKIP_datetime.fromtimestamp', type(e).__name__, e)

# === datetime.utcfromtimestamp ===
try:
    import time as time_module
    ts = time_module.time()
    dt_from_ts_utc = datetime.utcfromtimestamp(ts)
    print('dt_utcfromtimestamp_type', type(dt_from_ts_utc) is datetime)
except Exception as e:
    print('SKIP_datetime.utcfromtimestamp', type(e).__name__, e)

# === datetime.fromordinal ===
try:
    dt_from_ord = datetime.fromordinal(738886)
    print('dt_fromordinal_year', dt_from_ord.year)
except Exception as e:
    print('SKIP_datetime.fromordinal', type(e).__name__, e)

# === datetime.fromisoformat ===
try:
    dt_from_iso = datetime.fromisoformat('2024-03-15T14:30:45')
    print('dt_fromisoformat_hour', dt_from_iso.hour)
except Exception as e:
    print('SKIP_datetime.fromisoformat', type(e).__name__, e)

# === datetime attributes ===
try:
    dt = datetime(2023, 6, 15, 9, 30, 15, 123456)
    print('dt_attr_year', dt.year)
    print('dt_attr_month', dt.month)
    print('dt_attr_day', dt.day)
    print('dt_attr_hour', dt.hour)
    print('dt_attr_minute', dt.minute)
    print('dt_attr_second', dt.second)
    print('dt_attr_microsecond', dt.microsecond)
except Exception as e:
    print('SKIP_datetime_attributes', type(e).__name__, e)

# === datetime.date() and time() ===
try:
    dt = datetime(2024, 3, 15, 14, 30, 45)
    d_part = dt.date()
    t_part = dt.time()
    print('dt_date', type(d_part) is date)
    print('dt_date_year', d_part.year)
    print('dt_time', type(t_part) is time)
    print('dt_time_hour', t_part.hour)
except Exception as e:
    print('SKIP_datetime.date()_and_time()', type(e).__name__, e)

# === datetime.timetz ===
try:
    dt = datetime(2024, 3, 15, 14, 30, 45, tzinfo=timezone.utc)
    t_part_tz = dt.timetz()
    print('dt_timetz', type(t_part_tz) is time)
except Exception as e:
    print('SKIP_datetime.timetz', type(e).__name__, e)

# === datetime arithmetic - add timedelta ===
try:
    dt = datetime(2024, 1, 15, 12, 0, 0)
    td = timedelta(hours=5)
    result = dt + td
    print('dt_add_hour', result.hour)
    print('dt_add_day_unchanged', result.day)

    # Day boundary crossing
    dt = datetime(2024, 1, 15, 22, 0, 0)
    result = dt + timedelta(hours=5)
    print('dt_add_cross_day', result.day)
    print('dt_add_cross_hour', result.hour)
except Exception as e:
    print('SKIP_datetime_arithmetic_-_add_timedelta', type(e).__name__, e)

# === datetime arithmetic - subtract timedelta ===
try:
    dt = datetime(2024, 1, 15, 8, 0, 0)
    result = dt - timedelta(hours=3)
    print('dt_sub_hour', result.hour)

    # Day boundary crossing (subtract)
    dt = datetime(2024, 1, 15, 2, 0, 0)
    result = dt - timedelta(hours=5)
    print('dt_sub_cross_day', result.day)
    print('dt_sub_cross_hour', result.hour)
except Exception as e:
    print('SKIP_datetime_arithmetic_-_subtract_timedelta', type(e).__name__, e)

# === datetime isoformat ===
try:
    print('dt_isoformat_basic', datetime(2024, 3, 15, 14, 30, 45).isoformat())
    print('dt_isoformat_micro', datetime(2024, 3, 15, 14, 30, 45, 123456).isoformat())
except Exception as e:
    print('SKIP_datetime_isoformat', type(e).__name__, e)

# === datetime ctime ===
try:
    print('dt_ctime', datetime(2024, 3, 15, 14, 30, 45).ctime())
except Exception as e:
    print('SKIP_datetime_ctime', type(e).__name__, e)

# === datetime strftime ===
try:
    print('dt_strftime', datetime(2024, 3, 15, 14, 30, 45).strftime('%Y-%m-%d %H:%M:%S'))
except Exception as e:
    print('SKIP_datetime_strftime', type(e).__name__, e)

# === datetime timestamp ===
try:
    import math
    dt = datetime(2024, 1, 15, 12, 0, 0)
    ts = dt.timestamp()
    print('dt_timestamp_type', type(ts) is float)
    print('dt_timestamp_finite', math.isfinite(ts))
except Exception as e:
    print('SKIP_datetime_timestamp', type(e).__name__, e)

# === datetime timetuple ===
try:
    dt = datetime(2024, 1, 15, 12, 0, 0)
    tt = dt.timetuple()
    print('dt_timetuple_hour', tt.tm_hour)
except Exception as e:
    print('SKIP_datetime_timetuple', type(e).__name__, e)

# === datetime utctimetuple ===
try:
    dt = datetime(2024, 1, 15, 12, 0, 0)
    utt = dt.utctimetuple()
    print('dt_utctimetuple_hour', utt.tm_hour)
except Exception as e:
    print('SKIP_datetime_utctimetuple', type(e).__name__, e)

# === datetime replace ===
try:
    dt = datetime(2024, 3, 15, 14, 30, 45, 500000)
    dt_replaced = dt.replace(year=2025)
    print('dt_replace_year', dt_replaced.year)
    dt_replaced2 = dt.replace(hour=10)
    print('dt_replace_hour', dt_replaced2.hour)
    dt_replaced3 = dt.replace(microsecond=0)
    print('dt_replace_microsecond', dt_replaced3.microsecond)
except Exception as e:
    print('SKIP_datetime_replace', type(e).__name__, e)

# === datetime weekday/isoweekday/isocalendar ===
try:
    dt = datetime(2024, 1, 15, 14, 30, 45)
    print('dt_weekday', dt.weekday())
    print('dt_isoweekday', dt.isoweekday())
    dt_ica = dt.isocalendar()
    print('dt_isocalendar_week', dt_ica.week)
except Exception as e:
    print('SKIP_datetime_weekday/isoweekday/isocalendar', type(e).__name__, e)

# === time construction ===
try:
    t = time(14, 30, 45, 500000)
    print('time_hour', t.hour)
    print('time_minute', t.minute)
    print('time_second', t.second)
    print('time_microsecond', t.microsecond)
except Exception as e:
    print('SKIP_time_construction', type(e).__name__, e)

# === time construction - defaults ===
try:
    t = time(9, 0)
    print('time_default_second', t.second)
    print('time_default_microsecond', t.microsecond)

    t = time()
    print('time_all_default_hour', t.hour)
    print('time_all_default_minute', t.minute)
except Exception as e:
    print('SKIP_time_construction_-_defaults', type(e).__name__, e)

# === time.fromisoformat ===
try:
    t_from_iso = time.fromisoformat('14:30:45')
    print('time_fromisoformat_hour', t_from_iso.hour)
    t_from_iso2 = time.fromisoformat('14:30:45.123456')
    print('time_fromisoformat_microsecond', t_from_iso2.microsecond)
except Exception as e:
    print('SKIP_time.fromisoformat', type(e).__name__, e)

# === time attributes ===
try:
    t = time(23, 59, 59, 999999)
    print('time_attr_hour', t.hour)
    print('time_attr_minute', t.minute)
    print('time_attr_second', t.second)
    print('time_attr_microsecond', t.microsecond)
except Exception as e:
    print('SKIP_time_attributes', type(e).__name__, e)

# === time isoformat ===
try:
    print('time_isoformat', time(14, 30, 45).isoformat())
    print('time_isoformat_micro', time(14, 30, 45, 123456).isoformat())
except Exception as e:
    print('SKIP_time_isoformat', type(e).__name__, e)

# === time strftime ===
try:
    print('time_strftime', time(14, 30, 45).strftime('%H:%M:%S'))
except Exception as e:
    print('SKIP_time_strftime', type(e).__name__, e)

# === time replace ===
try:
    t = time(14, 30, 45, 500000)
    t_replaced = t.replace(hour=10)
    print('time_replace_hour', t_replaced.hour)
    t_replaced2 = t.replace(minute=0)
    print('time_replace_minute', t_replaced2.minute)
except Exception as e:
    print('SKIP_time_replace', type(e).__name__, e)

# === timezone UTC ===
try:
    print('UTC_is_utc', UTC.utc is None or UTC == UTC)
    print('UTC_utcoffset', UTC.utcoffset(datetime(2024, 1, 1)) == timedelta(0))
    print('UTC_tzname', UTC.tzname(datetime(2024, 1, 1)) == 'UTC')
except Exception as e:
    print('SKIP_timezone_UTC', type(e).__name__, e)

# === timezone custom ===
try:
    tz_plus2 = timezone(timedelta(hours=2))
    print('tz_plus2_utcoffset', tz_plus2.utcoffset(datetime(2024, 1, 1)) == timedelta(hours=2))

    tz_minus5 = timezone(timedelta(hours=-5))
    print('tz_minus5_utcoffset_hours', tz_minus5.utcoffset(datetime(2024, 1, 1)).seconds // 3600)

    tz_30min = timezone(timedelta(minutes=30))
    print('tz_30min_utcoffset_seconds', tz_30min.utcoffset(datetime(2024, 1, 1)).seconds)
except Exception as e:
    print('SKIP_timezone_custom', type(e).__name__, e)

# === timezone with name ===
try:
    tz_named = timezone(timedelta(hours=2), 'Europe/Paris')
    print('tz_named_tzname', tz_named.tzname(datetime(2024, 1, 1)))
except Exception as e:
    print('SKIP_timezone_with_name', type(e).__name__, e)

# === timezone dst ===
try:
    tz_plus2 = timezone(timedelta(hours=2))
    print('tz_utc_dst', UTC.dst(datetime(2024, 1, 1)))
    print('tz_plus2_dst', tz_plus2.dst(datetime(2024, 1, 1)))
except Exception as e:
    print('SKIP_timezone_dst', type(e).__name__, e)

# === tzinfo base class ===
try:
    print('tzinfo_is_class', type(tzinfo) is type)
    print('tzinfo_abstract', hasattr(tzinfo, 'utcoffset'))
except Exception as e:
    print('SKIP_tzinfo_base_class', type(e).__name__, e)

# === Edge cases - leap year ===
try:
    # Feb 29, 2024 (leap year)
    leap_date = date(2024, 2, 29)
    print('leap_2024_feb29_day', leap_date.day)
    print('leap_2024_feb29_month', leap_date.month)

    # Day after Feb 29
    dt_leap = datetime(2024, 2, 29, 23, 0, 0) + timedelta(hours=2)
    print('leap_next_day', dt_leap.day)
    print('leap_next_month', dt_leap.month)
except Exception as e:
    print('SKIP_Edge_cases_-_leap_year', type(e).__name__, e)

# === Edge cases - last day of month ===
try:
    # Last day of various months
    dec31 = date(2024, 12, 31)
    print('last_day_dec_day', dec31.day)
    print('last_day_dec_month', dec31.month)

    jan31 = date(2024, 1, 31)
    next_day = jan31 + timedelta(days=1)
    print('jan31_plus1_month', next_day.month)
    print('jan31_plus1_day', next_day.day)

    # Feb last day (non-leap)
    feb28 = date(2023, 2, 28)
    next_day = feb28 + timedelta(days=1)
    print('feb28_2023_plus1_month', next_day.month)
except Exception as e:
    print('SKIP_Edge_cases_-_last_day_of_month', type(e).__name__, e)

# === Edge cases - large timedelta ===
try:
    large_td = timedelta(days=9999)
    print('large_td_days', large_td.days)
    print('large_td_total_seconds_gt', large_td.total_seconds() > 800000000)
except Exception as e:
    print('SKIP_Edge_cases_-_large_timedelta', type(e).__name__, e)

# === Edge cases - date bounds ===
try:
    min_date = date(MINYEAR, 1, 1)
    print('min_date_year', min_date.year)

    max_date = date(MAXYEAR, 12, 31)
    print('max_date_year', max_date.year)
except Exception as e:
    print('SKIP_Edge_cases_-_date_bounds', type(e).__name__, e)

# === timedelta normalization ===
try:
    # 1 day + 25 hours should normalize properly
    td = timedelta(days=1, hours=25)
    print('td_normalize_days', td.days)
    print('td_normalize_seconds', td.seconds)

    # 1.5 seconds should become 1 second + 500000 microseconds
    td = timedelta(seconds=1, microseconds=500000)
    print('td_1_5_sec_seconds', td.seconds)
    print('td_1_5_sec_microseconds', td.microseconds)
except Exception as e:
    print('SKIP_timedelta_normalization', type(e).__name__, e)

# === timedelta from weeks ===
try:
    td_weeks = timedelta(weeks=2)
    print('td_weeks_days', td_weeks.days)
except Exception as e:
    print('SKIP_timedelta_from_weeks', type(e).__name__, e)

# === timedelta from milliseconds ===
try:
    td_ms = timedelta(milliseconds=1500)
    print('td_ms_seconds', td_ms.seconds)
    print('td_ms_microseconds', td_ms.microseconds)
except Exception as e:
    print('SKIP_timedelta_from_milliseconds', type(e).__name__, e)
