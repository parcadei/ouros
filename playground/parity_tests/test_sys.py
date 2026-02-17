"""Comprehensive parity test for Python's sys module.

Tests 100% of the safe, read-only API surface of the sys module.
Each attribute/function is tested with a unique snake_case label.
"""
import sys

# === version info attributes ===
try:
    print('sys_version_exists', hasattr(sys, 'version'))
    print('sys_version_type', type(sys.version).__name__)
    print('sys_version_info_exists', hasattr(sys, 'version_info'))
    print('sys_version_info_type', type(sys.version_info).__name__)
    print('sys_version_info_len', len(sys.version_info))
    print('sys_version_info_major', sys.version_info.major)
    print('sys_version_info_minor', sys.version_info.minor)
    print('sys_version_info_micro', sys.version_info.micro)
    print('sys_version_info_releaselevel', sys.version_info.releaselevel)
    print('sys_version_info_serial', sys.version_info.serial)
    print('sys_hexversion_exists', hasattr(sys, 'hexversion'))
    print('sys_hexversion_type', type(sys.hexversion).__name__)
except Exception as e:
    print('SKIP_version_info_attributes', type(e).__name__, e)

# === platform and implementation ===
try:
    print('sys_platform_exists', hasattr(sys, 'platform'))
    print('sys_platform_type', type(sys.platform).__name__)
    print('sys_platform_value', sys.platform)
    print('sys_implementation_exists', hasattr(sys, 'implementation'))
    print('sys_implementation_name', sys.implementation.name)
    print('sys_implementation_version', sys.implementation.version)
except Exception as e:
    print('SKIP_platform_and_implementation', type(e).__name__, e)

# === path and modules ===
try:
    print('sys_path_exists', hasattr(sys, 'path'))
    print('sys_path_type', type(sys.path).__name__)
    print('sys_path_len', len(sys.path))
    print('sys_modules_exists', hasattr(sys, 'modules'))
    print('sys_modules_type', type(sys.modules).__name__)
    print('sys_modules_len', len(sys.modules))
    print('sys_builtin_module_names_exists', hasattr(sys, 'builtin_module_names'))
    print('sys_builtin_module_names_type', type(sys.builtin_module_names).__name__)
    print('sys_stdlib_module_names_exists', hasattr(sys, 'stdlib_module_names'))
    print('sys_stdlib_module_names_type', type(sys.stdlib_module_names).__name__)
    print('sys_meta_path_exists', hasattr(sys, 'meta_path'))
    print('sys_meta_path_type', type(sys.meta_path).__name__)
    print('sys_path_hooks_exists', hasattr(sys, 'path_hooks'))
    print('sys_path_hooks_type', type(sys.path_hooks).__name__)
    print('sys_path_importer_cache_exists', hasattr(sys, 'path_importer_cache'))
    print('sys_path_importer_cache_type', type(sys.path_importer_cache).__name__)
except Exception as e:
    print('SKIP_path_and_modules', type(e).__name__, e)

# === executable and prefix info ===
try:
    print('sys_executable_exists', hasattr(sys, 'executable'))
    print('sys_executable_type', type(sys.executable).__name__)
    print('sys_prefix_exists', hasattr(sys, 'prefix'))
    print('sys_prefix_type', type(sys.prefix).__name__)
    print('sys_exec_prefix_exists', hasattr(sys, 'exec_prefix'))
    print('sys_exec_prefix_type', type(sys.exec_prefix).__name__)
    print('sys_base_prefix_exists', hasattr(sys, 'base_prefix'))
    print('sys_base_prefix_type', type(sys.base_prefix).__name__)
    print('sys_base_exec_prefix_exists', hasattr(sys, 'base_exec_prefix'))
    print('sys_base_exec_prefix_type', type(sys.base_exec_prefix).__name__)
    print('sys_platlibdir_exists', hasattr(sys, 'platlibdir'))
    print('sys_platlibdir_type', type(sys.platlibdir).__name__)
except Exception as e:
    print('SKIP_executable_and_prefix_info', type(e).__name__, e)

# === byteorder ===
try:
    print('sys_byteorder_exists', hasattr(sys, 'byteorder'))
    print('sys_byteorder_type', type(sys.byteorder).__name__)
    print('sys_byteorder_value', sys.byteorder)
except Exception as e:
    print('SKIP_byteorder', type(e).__name__, e)

# === max values ===
try:
    print('sys_maxsize_exists', hasattr(sys, 'maxsize'))
    print('sys_maxsize_type', type(sys.maxsize).__name__)
    print('sys_maxunicode_exists', hasattr(sys, 'maxunicode'))
    print('sys_maxunicode_type', type(sys.maxunicode).__name__)
except Exception as e:
    print('SKIP_max_values', type(e).__name__, e)

# === argv and orig_argv ===
try:
    print('sys_argv_exists', hasattr(sys, 'argv'))
    print('sys_argv_type', type(sys.argv).__name__)
    print('sys_orig_argv_exists', hasattr(sys, 'orig_argv'))
    print('sys_orig_argv_type', type(sys.orig_argv).__name__)
except Exception as e:
    print('SKIP_argv_and_orig_argv', type(e).__name__, e)

# === standard streams ===
try:
    print('sys_stdin_exists', hasattr(sys, 'stdin'))
    print('sys_stdin_type', type(sys.stdin).__name__)
    print('sys_stdout_exists', hasattr(sys, 'stdout'))
    print('sys_stdout_type', type(sys.stdout).__name__)
    print('sys_stderr_exists', hasattr(sys, 'stderr'))
    print('sys_stderr_type', type(sys.stderr).__name__)
    print('sys_stdin_is_none', sys.stdin is None)
    print('sys_stdout_is_none', sys.stdout is None)
    print('sys_stderr_is_none', sys.stderr is None)
except Exception as e:
    print('SKIP_standard_streams', type(e).__name__, e)

# === display and exception hooks ===
try:
    print('sys_displayhook_exists', hasattr(sys, 'displayhook'))
    print('sys_displayhook_callable', callable(sys.displayhook))
    print('sys_excepthook_exists', hasattr(sys, 'excepthook'))
    print('sys_excepthook_callable', callable(sys.excepthook))
    print('sys_breakpointhook_exists', hasattr(sys, 'breakpointhook'))
    print('sys_breakpointhook_callable', callable(sys.breakpointhook))
    print('sys_unraisablehook_exists', hasattr(sys, 'unraisablehook'))
    print('sys_unraisablehook_callable', callable(sys.unraisablehook))
except Exception as e:
    print('SKIP_display_and_exception_hooks', type(e).__name__, e)

# === info structures ===
try:
    print('sys_float_info_exists', hasattr(sys, 'float_info'))
    print('sys_float_info_type', type(sys.float_info).__name__)
    print('sys_float_info_max', sys.float_info.max > 0)
    print('sys_float_info_min', sys.float_info.min > 0)
    print('sys_float_info_epsilon', sys.float_info.epsilon > 0)
    print('sys_float_info_dig', sys.float_info.dig)
    print('sys_float_info_mant_dig', sys.float_info.mant_dig)
    print('sys_float_info_radix', sys.float_info.radix)
    print('sys_float_info_rounds', sys.float_info.rounds)

    print('sys_int_info_exists', hasattr(sys, 'int_info'))
    print('sys_int_info_type', type(sys.int_info).__name__)
    print('sys_int_info_bits_per_digit', sys.int_info.bits_per_digit)
    print('sys_int_info_sizeof_digit', sys.int_info.sizeof_digit)

    print('sys_hash_info_exists', hasattr(sys, 'hash_info'))
    print('sys_hash_info_type', type(sys.hash_info).__name__)
    print('sys_hash_info_width', sys.hash_info.width)
    print('sys_hash_info_modulus', sys.hash_info.modulus != 0)
    print('sys_hash_info_inf_exists', hasattr(sys.hash_info, 'inf'))
    print('sys_hash_info_nan_exists', hasattr(sys.hash_info, 'nan'))
    print('sys_hash_info_imag_exists', hasattr(sys.hash_info, 'imag'))

    print('sys_thread_info_exists', hasattr(sys, 'thread_info'))
    print('sys_thread_info_type', type(sys.thread_info).__name__)
    print('sys_thread_info_name', sys.thread_info.name)

    print('sys_flags_exists', hasattr(sys, 'flags'))
    print('sys_flags_type', type(sys.flags).__name__)
except Exception as e:
    print('SKIP_info_structures', type(e).__name__, e)

# === recursion limit ===
try:
    print('sys_getrecursionlimit_exists', hasattr(sys, 'getrecursionlimit'))
    print('sys_getrecursionlimit_callable', callable(sys.getrecursionlimit))
    print('sys_getrecursionlimit_value', sys.getrecursionlimit() > 0)
    print('sys_setrecursionlimit_exists', hasattr(sys, 'setrecursionlimit'))
    print('sys_setrecursionlimit_callable', callable(sys.setrecursionlimit))
except Exception as e:
    print('SKIP_recursion_limit', type(e).__name__, e)

# === getsizeof ===
try:
    print('sys_getsizeof_exists', hasattr(sys, 'getsizeof'))
    print('sys_getsizeof_callable', callable(sys.getsizeof))
    print('sys_getsizeof_int', sys.getsizeof(42) > 0)
    print('sys_getsizeof_str', sys.getsizeof('hello') > 0)
    print('sys_getsizeof_list', sys.getsizeof([1, 2, 3]) > 0)
    print('sys_getsizeof_dict', sys.getsizeof({}) > 0)
except Exception as e:
    print('SKIP_getsizeof', type(e).__name__, e)

# === encoding functions ===
try:
    print('sys_getdefaultencoding_exists', hasattr(sys, 'getdefaultencoding'))
    print('sys_getdefaultencoding_callable', callable(sys.getdefaultencoding))
    print('sys_getdefaultencoding_value', sys.getdefaultencoding())
    print('sys_getfilesystemencoding_exists', hasattr(sys, 'getfilesystemencoding'))
    print('sys_getfilesystemencoding_callable', callable(sys.getfilesystemencoding))
    print('sys_getfilesystemencoding_value', sys.getfilesystemencoding())
    print('sys_getfilesystemencodeerrors_exists', hasattr(sys, 'getfilesystemencodeerrors'))
    print('sys_getfilesystemencodeerrors_callable', callable(sys.getfilesystemencodeerrors))
    print('sys_getfilesystemencodeerrors_value', sys.getfilesystemencodeerrors())
except Exception as e:
    print('SKIP_encoding_functions', type(e).__name__, e)

# === intern ===
try:
    print('sys_intern_exists', hasattr(sys, 'intern'))
    print('sys_intern_callable', callable(sys.intern))
    print('sys_intern_str', type(sys.intern('test')).__name__)
    print('sys_intern_identity', sys.intern('hello') is sys.intern('hello'))
except Exception as e:
    print('SKIP_intern', type(e).__name__, e)

# === refcount ===
try:
    print('sys_getrefcount_exists', hasattr(sys, 'getrefcount'))
    print('sys_getrefcount_callable', callable(sys.getrefcount))
    print('sys_getrefcount_int', sys.getrefcount(42) > 0)
except Exception as e:
    print('SKIP_refcount', type(e).__name__, e)

# === memory and allocation ===
try:
    print('sys_getallocatedblocks_exists', hasattr(sys, 'getallocatedblocks'))
    print('sys_getallocatedblocks_callable', callable(sys.getallocatedblocks))
    print('sys_getallocatedblocks_value', sys.getallocatedblocks() >= 0)
except Exception as e:
    print('SKIP_memory_and_allocation', type(e).__name__, e)

# === switch interval ===
try:
    print('sys_getswitchinterval_exists', hasattr(sys, 'getswitchinterval'))
    print('sys_getswitchinterval_callable', callable(sys.getswitchinterval))
    print('sys_getswitchinterval_value', sys.getswitchinterval() > 0)
    print('sys_setswitchinterval_exists', hasattr(sys, 'setswitchinterval'))
    print('sys_setswitchinterval_callable', callable(sys.setswitchinterval))
except Exception as e:
    print('SKIP_switch_interval', type(e).__name__, e)

# === int max str digits ===
try:
    print('sys_get_int_max_str_digits_exists', hasattr(sys, 'get_int_max_str_digits'))
    print('sys_get_int_max_str_digits_callable', callable(sys.get_int_max_str_digits))
    print('sys_set_int_max_str_digits_exists', hasattr(sys, 'set_int_max_str_digits'))
    print('sys_set_int_max_str_digits_callable', callable(sys.set_int_max_str_digits))
except Exception as e:
    print('SKIP_int_max_str_digits', type(e).__name__, e)

# === unicode interned size ===
try:
    print('sys_getunicodeinternedsize_exists', hasattr(sys, 'getunicodeinternedsize'))
    print('sys_getunicodeinternedsize_callable', callable(sys.getunicodeinternedsize))
    print('sys_getunicodeinternedsize_value', sys.getunicodeinternedsize() >= 0)
except Exception as e:
    print('SKIP_unicode_interned_size', type(e).__name__, e)

# === copyright ===
try:
    print('sys_copyright_exists', hasattr(sys, 'copyright'))
    print('sys_copyright_type', type(sys.copyright).__name__)
    print('sys_copyright_len', len(sys.copyright) > 0)
except Exception as e:
    print('SKIP_copyright', type(e).__name__, e)

# === float repr style ===
try:
    print('sys_float_repr_style_exists', hasattr(sys, 'float_repr_style'))
    print('sys_float_repr_style_type', type(sys.float_repr_style).__name__)
    print('sys_float_repr_style_value', sys.float_repr_style)
except Exception as e:
    print('SKIP_float_repr_style', type(e).__name__, e)

# === abi flags ===
try:
    print('sys_abiflags_exists', hasattr(sys, 'abiflags'))
    print('sys_abiflags_type', type(sys.abiflags).__name__)
except Exception as e:
    print('SKIP_abi_flags', type(e).__name__, e)

# === dont_write_bytecode ===
try:
    print('sys_dont_write_bytecode_exists', hasattr(sys, 'dont_write_bytecode'))
    print('sys_dont_write_bytecode_type', type(sys.dont_write_bytecode).__name__)
except Exception as e:
    print('SKIP_dont_write_bytecode', type(e).__name__, e)

# === pycache_prefix ===
try:
    print('sys_pycache_prefix_exists', hasattr(sys, 'pycache_prefix'))
except Exception as e:
    print('SKIP_pycache_prefix', type(e).__name__, e)

# === api_version ===
try:
    print('sys_api_version_exists', hasattr(sys, 'api_version'))
    print('sys_api_version_type', type(sys.api_version).__name__)
except Exception as e:
    print('SKIP_api_version', type(e).__name__, e)

# === warnoptions ===
try:
    print('sys_warnoptions_exists', hasattr(sys, 'warnoptions'))
    print('sys_warnoptions_type', type(sys.warnoptions).__name__)
except Exception as e:
    print('SKIP_warnoptions', type(e).__name__, e)

# === is_finalizing ===
try:
    print('sys_is_finalizing_exists', hasattr(sys, 'is_finalizing'))
    print('sys_is_finalizing_callable', callable(sys.is_finalizing))
    print('sys_is_finalizing_value', sys.is_finalizing() in (True, False))
except Exception as e:
    print('SKIP_is_finalizing', type(e).__name__, e)

# === exc_info ===
try:
    print('sys_exc_info_exists', hasattr(sys, 'exc_info'))
    print('sys_exc_info_callable', callable(sys.exc_info))
    exc_info_result = sys.exc_info()
    print('sys_exc_info_returns_tuple', type(exc_info_result).__name__)
    print('sys_exc_info_tuple_len', len(exc_info_result))
    print('sys_exc_info_no_exception', exc_info_result[0] is None)
except Exception as e:
    print('SKIP_exc_info', type(e).__name__, e)

# === exception (current exception) ===
try:
    print('sys_exception_exists', hasattr(sys, 'exception'))
    print('sys_exception_callable', callable(sys.exception))
except Exception as e:
    print('SKIP_exception_current_exception', type(e).__name__, e)

# === exit ===
try:
    print('sys_exit_exists', hasattr(sys, 'exit'))
    print('sys_exit_callable', callable(sys.exit))
except Exception as e:
    print('SKIP_exit', type(e).__name__, e)

# === call_tracing ===
try:
    print('sys_call_tracing_exists', hasattr(sys, 'call_tracing'))
    print('sys_call_tracing_callable', callable(sys.call_tracing))
    print('sys_call_tracing_test', sys.call_tracing(len, ([1, 2, 3],)))
except Exception as e:
    print('SKIP_call_tracing', type(e).__name__, e)

# === audit and addaudithook ===
try:
    print('sys_audit_exists', hasattr(sys, 'audit'))
    print('sys_audit_callable', callable(sys.audit))
    print('sys_addaudithook_exists', hasattr(sys, 'addaudithook'))
    print('sys_addaudithook_callable', callable(sys.addaudithook))
except Exception as e:
    print('SKIP_audit_and_addaudithook', type(e).__name__, e)

# === getprofile and setprofile ===
try:
    print('sys_getprofile_exists', hasattr(sys, 'getprofile'))
    print('sys_getprofile_callable', callable(sys.getprofile))
    print('sys_setprofile_exists', hasattr(sys, 'setprofile'))
    print('sys_setprofile_callable', callable(sys.setprofile))
except Exception as e:
    print('SKIP_getprofile_and_setprofile', type(e).__name__, e)

# === gettrace and settrace ===
try:
    print('sys_gettrace_exists', hasattr(sys, 'gettrace'))
    print('sys_gettrace_callable', callable(sys.gettrace))
    print('sys_settrace_exists', hasattr(sys, 'settrace'))
    print('sys_settrace_callable', callable(sys.settrace))
except Exception as e:
    print('SKIP_gettrace_and_settrace', type(e).__name__, e)

# === get_asyncgen_hooks and set_asyncgen_hooks ===
try:
    print('sys_get_asyncgen_hooks_exists', hasattr(sys, 'get_asyncgen_hooks'))
    print('sys_get_asyncgen_hooks_callable', callable(sys.get_asyncgen_hooks))
    print('sys_set_asyncgen_hooks_exists', hasattr(sys, 'set_asyncgen_hooks'))
    print('sys_set_asyncgen_hooks_callable', callable(sys.set_asyncgen_hooks))
except Exception as e:
    print('SKIP_get_asyncgen_hooks_and_set_asyncgen_hooks', type(e).__name__, e)

# === get_coroutine_origin_tracking_depth and set_coroutine_origin_tracking_depth ===
try:
    print('sys_get_coroutine_origin_tracking_depth_exists', hasattr(sys, 'get_coroutine_origin_tracking_depth'))
    print('sys_get_coroutine_origin_tracking_depth_callable', callable(sys.get_coroutine_origin_tracking_depth))
    print('sys_set_coroutine_origin_tracking_depth_exists', hasattr(sys, 'set_coroutine_origin_tracking_depth'))
    print('sys_set_coroutine_origin_tracking_depth_callable', callable(sys.set_coroutine_origin_tracking_depth))
except Exception as e:
    print('SKIP_get_coroutine_origin_tracking_depth_and_set_coroutine_origin_tracking_depth', type(e).__name__, e)

# === monitoring ===
try:
    print('sys_monitoring_exists', hasattr(sys, 'monitoring'))
    print('sys_monitoring_type', type(sys.monitoring).__name__)
except Exception as e:
    print('SKIP_monitoring', type(e).__name__, e)

# === stack trampoline functions (3.12+) ===
try:
    print('sys_activate_stack_trampoline_exists', hasattr(sys, 'activate_stack_trampoline'))
    print('sys_deactivate_stack_trampoline_exists', hasattr(sys, 'deactivate_stack_trampoline'))
    print('sys_is_stack_trampoline_active_exists', hasattr(sys, 'is_stack_trampoline_active'))
except Exception as e:
    print('SKIP_stack_trampoline_functions_3.12+', type(e).__name__, e)

# === remote debug (3.14+) ===
try:
    print('sys_is_remote_debug_enabled_exists', hasattr(sys, 'is_remote_debug_enabled'))
    print('sys_is_remote_debug_enabled_callable', callable(sys.is_remote_debug_enabled))
    print('sys_remote_exec_exists', hasattr(sys, 'remote_exec'))
    print('sys_remote_exec_callable', callable(sys.remote_exec))
except Exception as e:
    print('SKIP_remote_debug_3.14+', type(e).__name__, e)

# === getdlopenflags and setdlopenflags (Unix only) ===
try:
    print('sys_getdlopenflags_exists', hasattr(sys, 'getdlopenflags'))
    print('sys_setdlopenflags_exists', hasattr(sys, 'setdlopenflags'))
except Exception as e:
    print('SKIP_getdlopenflags_and_setdlopenflags_Unix_only', type(e).__name__, e)

# === module complete ===
print('sys_module_test_complete', True)
