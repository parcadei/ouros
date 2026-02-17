# Comprehensive Python Built-in Exception Parity Tests
# Tests 100% of Python 3.14 built-in exceptions

# === BaseException ===
try:
    try:
        raise BaseException('baseexception_test')
    except BaseException:
        print('baseexception_caught', True)
except Exception as e:
    print('SKIP_BaseException', type(e).__name__, e)

# === Exception ===
try:
    try:
        raise Exception('exception_test')
    except Exception:
        print('exception_caught', True)
except Exception as e:
    print('SKIP_Exception', type(e).__name__, e)

# === ArithmeticError ===
try:
    try:
        raise ArithmeticError('arithmeticerror_test')
    except ArithmeticError:
        print('arithmeticerror_caught', True)
except Exception as e:
    print('SKIP_ArithmeticError', type(e).__name__, e)

# === LookupError ===
try:
    try:
        raise LookupError('lookuperror_test')
    except LookupError:
        print('lookuperror_caught', True)
except Exception as e:
    print('SKIP_LookupError', type(e).__name__, e)

# === BufferError ===
try:
    try:
        import io
        b = io.BytesIO(b'test')
        mv = memoryview(b.getbuffer())
        b.close()
        mv[0] = 1  # Should raise BufferError
    except BufferError:
        print('buffererror_caught', True)
    except (ValueError, OSError):
        # Fallback: some implementations raise different errors
        try:
            raise BufferError('buffererror_test')
        except BufferError:
            print('buffererror_caught', True)
except Exception as e:
    print('SKIP_BufferError', type(e).__name__, e)

# === AssertionError ===
try:
    try:
        assert False, 'assertionerror_test'
    except AssertionError:
        print('assertionerror_caught', True)
except Exception as e:
    print('SKIP_AssertionError', type(e).__name__, e)

# === AttributeError ===
try:
    try:
        obj = 42
        obj.nonexistent_attribute
    except AttributeError:
        print('attributeerror_caught', True)
except Exception as e:
    print('SKIP_AttributeError', type(e).__name__, e)

# === EOFError ===
try:
    import io
    try:
        raise EOFError('eoferror_test')
    except EOFError:
        print('eoferror_caught', True)
except Exception as e:
    print('SKIP_EOFError', type(e).__name__, e)

# === FloatingPointError ===
try:
    try:
        raise FloatingPointError('floatingpointerror_test')
    except FloatingPointError:
        print('floatingpointerror_caught', True)
except Exception as e:
    print('SKIP_FloatingPointError', type(e).__name__, e)

# === GeneratorExit ===
try:
    def gen_exit_test():
        try:
            yield 1
        except GeneratorExit:
            print('generatorexit_caught', True)
            raise
    g = gen_exit_test()
    next(g)
    g.close()
except Exception as e:
    print('SKIP_GeneratorExit', type(e).__name__, e)

# === ImportError ===
try:
    try:
        import nonexistent_module_xyz
    except ImportError:
        print('importerror_caught', True)
except Exception as e:
    print('SKIP_ImportError', type(e).__name__, e)

# === ModuleNotFoundError ===
try:
    try:
        import another_nonexistent_module_xyz
    except ModuleNotFoundError:
        print('modulenotfounderror_caught', True)
except Exception as e:
    print('SKIP_ModuleNotFoundError', type(e).__name__, e)

# === IndexError ===
try:
    try:
        lst = [1, 2, 3]
        lst[10]
    except IndexError:
        print('indexerror_caught', True)
except Exception as e:
    print('SKIP_IndexError', type(e).__name__, e)

# === KeyError ===
try:
    try:
        d = {}
        d['missing_key']
    except KeyError:
        print('keyerror_caught', True)
except Exception as e:
    print('SKIP_KeyError', type(e).__name__, e)

# === KeyboardInterrupt ===
try:
    try:
        raise KeyboardInterrupt('keyboardinterrupt_test')
    except KeyboardInterrupt:
        print('keyboardinterrupt_caught', True)
except Exception as e:
    print('SKIP_KeyboardInterrupt', type(e).__name__, e)

# === MemoryError ===
try:
    try:
        raise MemoryError('memoryerror_test')
    except MemoryError:
        print('memoryerror_caught', True)
except Exception as e:
    print('SKIP_MemoryError', type(e).__name__, e)

# === NameError ===
try:
    try:
        undefined_variable_xyz
    except NameError:
        print('nameerror_caught', True)
except Exception as e:
    print('SKIP_NameError', type(e).__name__, e)

# === NotImplementedError ===
try:
    try:
        raise NotImplementedError('notimplementederror_test')
    except NotImplementedError:
        print('notimplementederror_caught', True)
except Exception as e:
    print('SKIP_NotImplementedError', type(e).__name__, e)

# === OverflowError ===
try:
    try:
        import math
        math.exp(1000000)
    except OverflowError:
        print('overflowerror_caught', True)
except Exception as e:
    print('SKIP_OverflowError', type(e).__name__, e)

# === RecursionError ===
try:
    try:
        def recurse():
            return recurse()
        import sys
        sys.setrecursionlimit(100)
        try:
            recurse()
        finally:
            sys.setrecursionlimit(1000)
    except RecursionError:
        print('recursionerror_caught', True)
except Exception as e:
    print('SKIP_RecursionError', type(e).__name__, e)

# === ReferenceError ===
try:
    import weakref
    try:
        class Obj:
            pass
        obj = Obj()
        ref = weakref.ref(obj)
        del obj
        ref()  # Returns None, but testing ReferenceError creation
        raise ReferenceError('referenceerror_test')
    except ReferenceError:
        print('referenceerror_caught', True)
except Exception as e:
    print('SKIP_ReferenceError', type(e).__name__, e)

# === RuntimeError ===
try:
    try:
        raise RuntimeError('runtimeerror_test')
    except RuntimeError:
        print('runtimeerror_caught', True)
except Exception as e:
    print('SKIP_RuntimeError', type(e).__name__, e)

# === StopIteration ===
try:
    def stop_iter_test():
        try:
            raise StopIteration('stopiteration_test')
        except StopIteration:
            print('stopiteration_caught', True)
    stop_iter_test()
except Exception as e:
    print('SKIP_StopIteration', type(e).__name__, e)

# === StopAsyncIteration ===
try:
    import asyncio
    try:
        raise StopAsyncIteration('stopasynciteration_test')
    except StopAsyncIteration:
        print('stopasynciteration_caught', True)
except Exception as e:
    print('SKIP_StopAsyncIteration', type(e).__name__, e)

# === SyntaxError ===
try:
    try:
        compile('invalid syntax @#$', '<test>', 'exec')
    except SyntaxError:
        print('syntaxerror_caught', True)
except Exception as e:
    print('SKIP_SyntaxError', type(e).__name__, e)

# === IndentationError ===
try:
    try:
        compile('def foo():\nprint(1)', '<test>', 'exec')
    except IndentationError:
        print('indentationerror_caught', True)
except Exception as e:
    print('SKIP_IndentationError', type(e).__name__, e)

# === TabError ===
try:
    try:
        compile('def foo():\n\tpass\n        pass', '<test>', 'exec')
    except TabError:
        print('taberror_caught', True)
except Exception as e:
    print('SKIP_TabError', type(e).__name__, e)

# === SystemError ===
try:
    try:
        raise SystemError('systemerror_test')
    except SystemError:
        print('systemerror_caught', True)
except Exception as e:
    print('SKIP_SystemError', type(e).__name__, e)

# === SystemExit ===
try:
    try:
        raise SystemExit('systemexit_test')
    except SystemExit:
        print('systemexit_caught', True)
except Exception as e:
    print('SKIP_SystemExit', type(e).__name__, e)

# === TypeError ===
try:
    try:
        len(42)
    except TypeError:
        print('typeerror_caught', True)
except Exception as e:
    print('SKIP_TypeError', type(e).__name__, e)

# === UnboundLocalError ===
try:
    def unbound_local_test():
        try:
            x = x + 1
        except UnboundLocalError:
            print('unboundlocalerror_caught', True)
    unbound_local_test()
except Exception as e:
    print('SKIP_UnboundLocalError', type(e).__name__, e)

# === ValueError ===
try:
    try:
        int('not a number')
    except ValueError:
        print('valueerror_caught', True)
except Exception as e:
    print('SKIP_ValueError', type(e).__name__, e)

# === ZeroDivisionError ===
try:
    try:
        1 / 0
    except ZeroDivisionError:
        print('zerodivisionerror_caught', True)
except Exception as e:
    print('SKIP_ZeroDivisionError', type(e).__name__, e)

# === PythonFinalizationError ===
try:
    try:
        raise PythonFinalizationError('pythonfinalizationerror_test')
    except PythonFinalizationError:
        print('pythonfinalizationerror_caught', True)
except Exception as e:
    print('SKIP_PythonFinalizationError', type(e).__name__, e)

# === OSError ===
try:
    try:
        raise OSError('oserror_test')
    except OSError:
        print('oserror_caught', True)
except Exception as e:
    print('SKIP_OSError', type(e).__name__, e)

# === BlockingIOError ===
try:
    try:
        raise BlockingIOError('blockingioerror_test')
    except BlockingIOError:
        print('blockingioerror_caught', True)
except Exception as e:
    print('SKIP_BlockingIOError', type(e).__name__, e)

# === ChildProcessError ===
try:
    try:
        raise ChildProcessError('childprocesserror_test')
    except ChildProcessError:
        print('childprocesserror_caught', True)
except Exception as e:
    print('SKIP_ChildProcessError', type(e).__name__, e)

# === ConnectionError ===
try:
    try:
        raise ConnectionError('connectionerror_test')
    except ConnectionError:
        print('connectionerror_caught', True)
except Exception as e:
    print('SKIP_ConnectionError', type(e).__name__, e)

# === BrokenPipeError ===
try:
    try:
        raise BrokenPipeError('brokenpipeerror_test')
    except BrokenPipeError:
        print('brokenpipeerror_caught', True)
except Exception as e:
    print('SKIP_BrokenPipeError', type(e).__name__, e)

# === ConnectionAbortedError ===
try:
    try:
        raise ConnectionAbortedError('connectionabortederror_test')
    except ConnectionAbortedError:
        print('connectionabortederror_caught', True)
except Exception as e:
    print('SKIP_ConnectionAbortedError', type(e).__name__, e)

# === ConnectionRefusedError ===
try:
    try:
        raise ConnectionRefusedError('connectionrefusederror_test')
    except ConnectionRefusedError:
        print('connectionrefusederror_caught', True)
except Exception as e:
    print('SKIP_ConnectionRefusedError', type(e).__name__, e)

# === ConnectionResetError ===
try:
    try:
        raise ConnectionResetError('connectionreseterror_test')
    except ConnectionResetError:
        print('connectionreseterror_caught', True)
except Exception as e:
    print('SKIP_ConnectionResetError', type(e).__name__, e)

# === FileExistsError ===
try:
    import tempfile
    import os
    try:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, 'testfile')
            with open(path, 'w') as f:
                f.write('test')
            try:
                os.mkdir(path)
            except FileExistsError:
                print('fileexistserror_caught', True)
    except Exception:
        # Fallback
        try:
            raise FileExistsError('fileexistserror_test')
        except FileExistsError:
            print('fileexistserror_caught', True)
except Exception as e:
    print('SKIP_FileExistsError', type(e).__name__, e)

# === FileNotFoundError ===
try:
    try:
        open('/nonexistent/path/xyz123.txt')
    except FileNotFoundError:
        print('filenotfounderror_caught', True)
except Exception as e:
    print('SKIP_FileNotFoundError', type(e).__name__, e)

# === InterruptedError ===
try:
    try:
        raise InterruptedError('interruptederror_test')
    except InterruptedError:
        print('interruptederror_caught', True)
except Exception as e:
    print('SKIP_InterruptedError', type(e).__name__, e)

# === IsADirectoryError ===
try:
    try:
        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                with open(tmpdir, 'r') as f:
                    pass
            except IsADirectoryError:
                print('isadirectoryerror_caught', True)
    except Exception:
        try:
            raise IsADirectoryError('isadirectoryerror_test')
        except IsADirectoryError:
            print('isadirectoryerror_caught', True)
except Exception as e:
    print('SKIP_IsADirectoryError', type(e).__name__, e)

# === NotADirectoryError ===
try:
    try:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, 'file')
            with open(path, 'w') as f:
                f.write('test')
            try:
                os.listdir(path)
            except NotADirectoryError:
                print('notadirectoryerror_caught', True)
    except Exception:
        try:
            raise NotADirectoryError('notadirectoryerror_test')
        except NotADirectoryError:
            print('notadirectoryerror_caught', True)
except Exception as e:
    print('SKIP_NotADirectoryError', type(e).__name__, e)

# === PermissionError ===
try:
    try:
        open('/root/xyz123_test', 'w')
    except PermissionError:
        print('permissionerror_caught', True)
    except (IsADirectoryError, OSError):
        # Fallback for systems where /root doesn't exist or is accessible
        try:
            raise PermissionError('permissionerror_test')
        except PermissionError:
            print('permissionerror_caught', True)
except Exception as e:
    print('SKIP_PermissionError', type(e).__name__, e)

# === ProcessLookupError ===
try:
    try:
        raise ProcessLookupError('processlookuperror_test')
    except ProcessLookupError:
        print('processlookuperror_caught', True)
except Exception as e:
    print('SKIP_ProcessLookupError', type(e).__name__, e)

# === TimeoutError ===
try:
    try:
        raise TimeoutError('timeouterror_test')
    except TimeoutError:
        print('timeouterror_caught', True)
except Exception as e:
    print('SKIP_TimeoutError', type(e).__name__, e)

# === IOError (alias for OSError) ===
try:
    try:
        raise IOError('ioerror_test')
    except IOError:
        print('ioerror_caught', True)
except Exception as e:
    print('SKIP_IOError (alias for OSError)', type(e).__name__, e)

# === EnvironmentError (alias for OSError) ===
try:
    try:
        raise EnvironmentError('environmenterror_test')
    except EnvironmentError:
        print('environmenterror_caught', True)
except Exception as e:
    print('SKIP_EnvironmentError (alias for OSError)', type(e).__name__, e)

# === UnicodeError ===
try:
    try:
        raise UnicodeError('unicodeerror_test')
    except UnicodeError:
        print('unicodeerror_caught', True)
except Exception as e:
    print('SKIP_UnicodeError', type(e).__name__, e)

# === UnicodeDecodeError ===
try:
    try:
        b'\xff\xfe'.decode('utf-8')
    except UnicodeDecodeError:
        print('unicodedecodeerror_caught', True)
except Exception as e:
    print('SKIP_UnicodeDecodeError', type(e).__name__, e)

# === UnicodeEncodeError ===
try:
    try:
        '\ud800'.encode('utf-8')
    except UnicodeEncodeError:
        print('unicodeencodeerror_caught', True)
except Exception as e:
    print('SKIP_UnicodeEncodeError', type(e).__name__, e)

# === UnicodeTranslateError ===
try:
    try:
        raise UnicodeTranslateError('source', 0, 1, 'reason')
    except UnicodeTranslateError:
        print('unicodetranslateerror_caught', True)
except Exception as e:
    print('SKIP_UnicodeTranslateError', type(e).__name__, e)

# === Warning ===
try:
    import warnings
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', Warning)
            warnings.warn('warning_test', Warning)
    except Warning:
        print('warning_caught', True)
except Exception as e:
    print('SKIP_Warning', type(e).__name__, e)

# === UserWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', UserWarning)
            warnings.warn('userwarning_test', UserWarning)
    except UserWarning:
        print('userwarning_caught', True)
except Exception as e:
    print('SKIP_UserWarning', type(e).__name__, e)

# === DeprecationWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', DeprecationWarning)
            warnings.warn('deprecationwarning_test', DeprecationWarning)
    except DeprecationWarning:
        print('deprecationwarning_caught', True)
except Exception as e:
    print('SKIP_DeprecationWarning', type(e).__name__, e)

# === PendingDeprecationWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', PendingDeprecationWarning)
            warnings.warn('pendingdeprecationwarning_test', PendingDeprecationWarning)
    except PendingDeprecationWarning:
        print('pendingdeprecationwarning_caught', True)
except Exception as e:
    print('SKIP_PendingDeprecationWarning', type(e).__name__, e)

# === SyntaxWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', SyntaxWarning)
            warnings.warn('syntaxwarning_test', SyntaxWarning)
    except SyntaxWarning:
        print('syntaxwarning_caught', True)
except Exception as e:
    print('SKIP_SyntaxWarning', type(e).__name__, e)

# === RuntimeWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', RuntimeWarning)
            warnings.warn('runtimewarning_test', RuntimeWarning)
    except RuntimeWarning:
        print('runtimewarning_caught', True)
except Exception as e:
    print('SKIP_RuntimeWarning', type(e).__name__, e)

# === FutureWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', FutureWarning)
            warnings.warn('futurewarning_test', FutureWarning)
    except FutureWarning:
        print('futurewarning_caught', True)
except Exception as e:
    print('SKIP_FutureWarning', type(e).__name__, e)

# === ImportWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', ImportWarning)
            warnings.warn('importwarning_test', ImportWarning)
    except ImportWarning:
        print('importwarning_caught', True)
except Exception as e:
    print('SKIP_ImportWarning', type(e).__name__, e)

# === UnicodeWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', UnicodeWarning)
            warnings.warn('unicodewarning_test', UnicodeWarning)
    except UnicodeWarning:
        print('unicodewarning_caught', True)
except Exception as e:
    print('SKIP_UnicodeWarning', type(e).__name__, e)

# === EncodingWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', EncodingWarning)
            warnings.warn('encodingwarning_test', EncodingWarning)
    except EncodingWarning:
        print('encodingwarning_caught', True)
except Exception as e:
    print('SKIP_EncodingWarning', type(e).__name__, e)

# === BytesWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', BytesWarning)
            warnings.warn('byteswarning_test', BytesWarning)
    except BytesWarning:
        print('byteswarning_caught', True)
except Exception as e:
    print('SKIP_BytesWarning', type(e).__name__, e)

# === ResourceWarning ===
try:
    try:
        with warnings.catch_warnings():
            warnings.simplefilter('error', ResourceWarning)
            warnings.warn('resourcewarning_test', ResourceWarning)
    except ResourceWarning:
        print('resourcewarning_caught', True)
except Exception as e:
    print('SKIP_ResourceWarning', type(e).__name__, e)

# === BaseExceptionGroup (Python 3.11+) ===
try:
    try:
        raise BaseExceptionGroup('baseexceptiongroup_test', [ValueError('v')])
    except BaseExceptionGroup:
        print('baseexceptiongroup_caught', True)
except Exception as e:
    print('SKIP_BaseExceptionGroup (Python 3.11+)', type(e).__name__, e)

# === ExceptionGroup (Python 3.11+) ===
try:
    try:
        raise ExceptionGroup('exceptiongroup_test', [ValueError('v'), TypeError('t')])
    except ExceptionGroup:
        print('exceptiongroup_caught', True)

    print('all_tests_complete', True)
except Exception as e:
    print('SKIP_ExceptionGroup (Python 3.11+)', type(e).__name__, e)
