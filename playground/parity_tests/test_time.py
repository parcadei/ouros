import time
import math

# === Module import ===
try:
    print('module_import', 'time' in dir())
except Exception as e:
    print('SKIP_Module_import', type(e).__name__, e)

# === time.time() ===
try:
    # Function is callable and returns a float
    t1 = time.time()
    print('time_callable', callable(time.time))
    print('time_returns_float', type(t1) is float)

    # Multiple calls return increasing values (relative to each other)
    t2 = time.time()
    t3 = time.time()
    print('time_increasing_1', t1 <= t2)
    print('time_increasing_2', t2 <= t3)
except Exception as e:
    print('SKIP_time.time()', type(e).__name__, e)

# === time.time_ns() ===
try:
    # Function is callable and returns an int
    ns1 = time.time_ns()
    print('time_ns_callable', callable(time.time_ns))
    print('time_ns_returns_int', type(ns1) is int)

    # Returns increasing values
    ns2 = time.time_ns()
    print('time_ns_increasing', ns1 <= ns2)

    # time_ns should be roughly time() * 1e9
    print('time_ns_scale', ns1 > 1000000000)
except Exception as e:
    print('SKIP_time.time_ns()', type(e).__name__, e)

# === time.monotonic() ===
try:
    # Function is callable and returns a float
    m1 = time.monotonic()
    print('monotonic_callable', callable(time.monotonic))
    print('monotonic_returns_float', type(m1) is float)

    # Returns increasing values
    m2 = time.monotonic()
    print('monotonic_increasing', m1 <= m2)
except Exception as e:
    print('SKIP_time.monotonic()', type(e).__name__, e)

# === time.monotonic_ns() ===
try:
    # Function is callable and returns an int
    ns1 = time.monotonic_ns()
    print('monotonic_ns_callable', callable(time.monotonic_ns))
    print('monotonic_ns_returns_int', type(ns1) is int)

    # Returns increasing values
    ns2 = time.monotonic_ns()
    print('monotonic_ns_increasing', ns1 <= ns2)
except Exception as e:
    print('SKIP_time.monotonic_ns()', type(e).__name__, e)

# === time.perf_counter() ===
try:
    # Function is callable and returns a float
    p1 = time.perf_counter()
    print('perf_counter_callable', callable(time.perf_counter))
    print('perf_counter_returns_float', type(p1) is float)

    # Returns increasing values
    p2 = time.perf_counter()
    print('perf_counter_increasing', p1 <= p2)
except Exception as e:
    print('SKIP_time.perf_counter()', type(e).__name__, e)

# === time.perf_counter_ns() ===
try:
    # Function is callable and returns an int
    pc_ns1 = time.perf_counter_ns()
    print('perf_counter_ns_callable', callable(time.perf_counter_ns))
    print('perf_counter_ns_returns_int', type(pc_ns1) is int)

    # Returns increasing values
    pc_ns2 = time.perf_counter_ns()
    print('perf_counter_ns_increasing', pc_ns1 <= pc_ns2)
except Exception as e:
    print('SKIP_time.perf_counter_ns()', type(e).__name__, e)

# === time.process_time() ===
try:
    # Function is callable and returns a float
    pt1 = time.process_time()
    print('process_time_callable', callable(time.process_time))
    print('process_time_returns_float', type(pt1) is float)

    # Returns non-negative values
    print('process_time_non_negative', pt1 >= 0)
except Exception as e:
    print('SKIP_time.process_time()', type(e).__name__, e)

# === time.process_time_ns() ===
try:
    # Function is callable and returns an int
    pt_ns1 = time.process_time_ns()
    print('process_time_ns_callable', callable(time.process_time_ns))
    print('process_time_ns_returns_int', type(pt_ns1) is int)

    # Returns non-negative values
    print('process_time_ns_non_negative', pt_ns1 >= 0)
except Exception as e:
    print('SKIP_time.process_time_ns()', type(e).__name__, e)

# === time.sleep() ===
try:
    # Raises RuntimeError when called in sandbox
    try:
        time.sleep(0.001)
        print('sleep_raises_error', False)
    except RuntimeError as e:
        print('sleep_raises_error', True)
        print('sleep_error_type', 'RuntimeError')
    except Exception as e:
        print('sleep_raises_error', type(e).__name__)
except Exception as e:
    print('SKIP_time.sleep()', type(e).__name__, e)

# === time.gmtime() ===
try:
    # Convert seconds since epoch to UTC time struct
    now = time.time()
    gmt = time.gmtime(now)
    print('gmtime_type', type(gmt).__name__)
    print('gmtime_has_tm_year', hasattr(gmt, 'tm_year'))
    print('gmtime_has_tm_mon', hasattr(gmt, 'tm_mon'))
    print('gmtime_has_tm_mday', hasattr(gmt, 'tm_mday'))
    print('gmtime_has_tm_hour', hasattr(gmt, 'tm_hour'))
    print('gmtime_has_tm_min', hasattr(gmt, 'tm_min'))
    print('gmtime_has_tm_sec', hasattr(gmt, 'tm_sec'))
    print('gmtime_has_tm_wday', hasattr(gmt, 'tm_wday'))
    print('gmtime_has_tm_yday', hasattr(gmt, 'tm_yday'))
    print('gmtime_has_tm_isdst', hasattr(gmt, 'tm_isdst'))

    # gmtime with 0 (epoch)
    epoch_gmt = time.gmtime(0)
    print('gmtime_epoch_year', epoch_gmt.tm_year)
    print('gmtime_epoch_mon', epoch_gmt.tm_mon)
    print('gmtime_epoch_mday', epoch_gmt.tm_mday)

    # gmtime() with no argument uses current time
    current_gmt = time.gmtime()
    print('gmtime_no_arg', current_gmt.tm_year > 2020)
except Exception as e:
    print('SKIP_time.gmtime()', type(e).__name__, e)

# === time.localtime() ===
try:
    # Convert seconds since epoch to local time struct
    lt = time.localtime(now)
    print('localtime_type', type(lt).__name__)
    print('localtime_has_tm_year', hasattr(lt, 'tm_year'))

    # localtime with no argument uses current time
    current_lt = time.localtime()
    print('localtime_no_arg', current_lt.tm_year > 2020)
except Exception as e:
    print('SKIP_time.localtime()', type(e).__name__, e)

# === time.mktime() ===
try:
    # Convert local time struct to seconds since epoch
    lt_now = time.localtime()
    mktime_result = time.mktime(lt_now)
    print('mktime_returns_float', type(mktime_result) is float)
    print('mktime_positive', mktime_result > 0)

    # mktime should round-trip with localtime
    lt2 = time.localtime(mktime_result)
    print('mktime_roundtrip_year', lt2.tm_year == lt_now.tm_year)
    print('mktime_roundtrip_mon', lt2.tm_mon == lt_now.tm_mon)
    print('mktime_roundtrip_mday', lt2.tm_mday == lt_now.tm_mday)
except Exception as e:
    print('SKIP_time.mktime()', type(e).__name__, e)

# === time.asctime() ===
try:
    # Convert time tuple to string
    asc = time.asctime(time.gmtime(0))
    print('asctype_returns_str', type(asc) is str)
    print('asctime_epoch', asc)

    # asctime with no argument uses current time
    current_asc = time.asctime()
    print('asctime_no_arg', len(current_asc) > 0)
except Exception as e:
    print('SKIP_time.asctime()', type(e).__name__, e)

# === time.ctime() ===
try:
    # Convert seconds since epoch to string
    ct = time.ctime(0)
    print('ctime_returns_str', type(ct) is str)
    print('ctime_epoch', ct)

    # ctime with no argument uses current time
    current_ct = time.ctime()
    print('ctime_no_arg', len(current_ct) > 0)

    # ctime should match asctime(gmtime())
    print('ctime_asctime_match', time.ctime(0) == time.asctime(time.gmtime(0)))
except Exception as e:
    print('SKIP_time.ctime()', type(e).__name__, e)

# === time.strftime() ===
try:
    # Format time as string
    fmt = '%Y-%m-%d %H:%M:%S'
    formatted = time.strftime(fmt, time.gmtime(0))
    print('strftime_returns_str', type(formatted) is str)
    print('strftime_epoch', formatted)

    # strftime with no time uses current time
    current_fmt = time.strftime(fmt)
    print('strftime_no_time', len(current_fmt) > 0)

    # Various format codes
    print('strftime_year', time.strftime('%Y', time.gmtime(0)))
    print('strftime_month', time.strftime('%m', time.gmtime(0)))
    print('strftime_day', time.strftime('%d', time.gmtime(0)))
except Exception as e:
    print('SKIP_time.strftime()', type(e).__name__, e)

# === time.strptime() ===
try:
    # Parse string to time tuple
    parsed = time.strptime('1970-01-01 00:00:00', '%Y-%m-%d %H:%M:%S')
    print('strptime_returns_struct', type(parsed).__name__)
    print('strptime_year', parsed.tm_year)
    print('strptime_mon', parsed.tm_mon)
    print('strptime_mday', parsed.tm_mday)
except Exception as e:
    print('SKIP_time.strptime()', type(e).__name__, e)

# === time.struct_time ===
try:
    # Check that struct_time is available
    print('struct_time_exists', hasattr(time, 'struct_time'))
    print('struct_time_is_type', type(time.struct_time) is type)

    # Create struct_time manually
    st = time.struct_time((2024, 3, 15, 14, 30, 0, 4, 75, 0))
    print('struct_time_year', st.tm_year)
    print('struct_time_mon', st.tm_mon)
    print('struct_time_mday', st.tm_mday)

    # struct_time is iterable
    st_list = list(st)
    print('struct_time_len', len(st_list))
    print('struct_time_index_0', st_list[0])

    # struct_time can be indexed
    print('struct_time_getitem', st[0])
except Exception as e:
    print('SKIP_time.struct_time', type(e).__name__, e)

# === time.get_clock_info() ===
try:
    # Get information about a clock
    info = time.get_clock_info('monotonic')
    print('get_clock_info_returns_namespace', type(info).__name__)
    print('get_clock_info_has_implementation', hasattr(info, 'implementation'))
    print('get_clock_info_has_monotonic', hasattr(info, 'monotonic'))
    print('get_clock_info_has_adjustable', hasattr(info, 'adjustable'))
    print('get_clock_info_has_resolution', hasattr(info, 'resolution'))

    # Try other clocks
    clocks = ['time', 'monotonic', 'perf_counter', 'process_time']
    for clock in clocks:
        try:
            info = time.get_clock_info(clock)
            print(f'get_clock_info_{clock}', 'success')
        except:
            print(f'get_clock_info_{clock}', 'not_available')
except Exception as e:
    print('SKIP_time.get_clock_info()', type(e).__name__, e)

# === time.thread_time() ===
try:
    try:
        tt = time.thread_time()
        print('thread_time_callable', True)
        print('thread_time_returns_number', type(tt) in (int, float))
        print('thread_time_non_negative', tt >= 0)
    except:
        print('thread_time_callable', False)
except Exception as e:
    print('SKIP_time.thread_time()', type(e).__name__, e)

# === time.thread_time_ns() ===
try:
    try:
        tt_ns = time.thread_time_ns()
        print('thread_time_ns_callable', True)
        print('thread_time_ns_returns_int', type(tt_ns) is int)
        print('thread_time_ns_non_negative', tt_ns >= 0)
    except:
        print('thread_time_ns_callable', False)
except Exception as e:
    print('SKIP_time.thread_time_ns()', type(e).__name__, e)

# === Monotonic property verification ===
try:
    # Verify sequence of calls maintains monotonicity
    a = time.time()
    b = time.monotonic()
    c = time.time()
    d = time.monotonic()

    # All values should be non-negative
    print('time_non_negative', a >= 0)
    print('monotonic_non_negative', b >= 0)

    # Verify monotonic_ns is in nanoseconds (larger than time in seconds scaled)
    ns_val = time.monotonic_ns()
    t_val = time.time()
    # ns should be roughly t * 1e9, but since sandbox returns different scales,
    # we just verify ns is a large integer and increasing
    print('monotonic_ns_large', ns_val > 1000000)
except Exception as e:
    print('SKIP_Monotonic_property_verification', type(e).__name__, e)

# === Constants ===
try:
    print('timezone_exists', hasattr(time, 'timezone'))
    print('altzone_exists', hasattr(time, 'altzone'))
    print('daylight_exists', hasattr(time, 'daylight'))
    print('tzname_exists', hasattr(time, 'tzname'))
except Exception as e:
    print('SKIP_Constants', type(e).__name__, e)

# === Clock constants ===
try:
    clock_constants = [
        'CLOCK_MONOTONIC',
        'CLOCK_MONOTONIC_RAW',
        'CLOCK_REALTIME',
        'CLOCK_PROCESS_CPUTIME_ID',
        'CLOCK_THREAD_CPUTIME_ID',
    ]
    for const in clock_constants:
        if hasattr(time, const):
            print(f'{const}_exists', True)
        else:
            print(f'{const}_exists', False)
except Exception as e:
    print('SKIP_Clock_constants', type(e).__name__, e)
