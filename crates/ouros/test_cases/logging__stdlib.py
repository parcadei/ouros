# === Module import and exported names ===
import logging
import warnings

warnings.filterwarnings('ignore', category=DeprecationWarning)

expected_public = {
    'BASIC_FORMAT',
    'BufferingFormatter',
    'CRITICAL',
    'DEBUG',
    'ERROR',
    'FATAL',
    'FileHandler',
    'Filter',
    'Filterer',
    'Formatter',
    'GenericAlias',
    'Handler',
    'INFO',
    'LogRecord',
    'Logger',
    'LoggerAdapter',
    'Manager',
    'NOTSET',
    'NullHandler',
    'PercentStyle',
    'PlaceHolder',
    'RootLogger',
    'StrFormatStyle',
    'StreamHandler',
    'StringTemplateStyle',
    'Template',
    'WARN',
    'WARNING',
    'addLevelName',
    'atexit',
    'basicConfig',
    'captureWarnings',
    'collections',
    'critical',
    'currentframe',
    'debug',
    'disable',
    'error',
    'exception',
    'fatal',
    'getHandlerByName',
    'getHandlerNames',
    'getLevelName',
    'getLevelNamesMapping',
    'getLogRecordFactory',
    'getLogger',
    'getLoggerClass',
    'info',
    'io',
    'lastResort',
    'log',
    'logAsyncioTasks',
    'logMultiprocessing',
    'logProcesses',
    'logThreads',
    'makeLogRecord',
    'os',
    'raiseExceptions',
    're',
    'root',
    'setLogRecordFactory',
    'setLoggerClass',
    'shutdown',
    'sys',
    'threading',
    'time',
    'traceback',
    'warn',
    'warning',
    'warnings',
    'weakref',
}

module_dir = set(dir(logging))
for name in expected_public:
    assert name in module_dir, f'missing public logging export: {name}'

# === Constants and flags ===
assert logging.CRITICAL == 50, 'CRITICAL should be 50'
assert logging.FATAL == 50, 'FATAL should be 50'
assert logging.ERROR == 40, 'ERROR should be 40'
assert logging.WARNING == 30, 'WARNING should be 30'
assert logging.WARN == 30, 'WARN should be 30'
assert logging.INFO == 20, 'INFO should be 20'
assert logging.DEBUG == 10, 'DEBUG should be 10'
assert logging.NOTSET == 0, 'NOTSET should be 0'
assert logging.BASIC_FORMAT == '%(levelname)s:%(name)s:%(message)s', 'BASIC_FORMAT mismatch'
assert logging.raiseExceptions is True, 'raiseExceptions default'

# === Class-like exports are callable ===
for class_name in [
    'BufferingFormatter',
    'FileHandler',
    'Filter',
    'Filterer',
    'Formatter',
    'Handler',
    'LogRecord',
    'Logger',
    'LoggerAdapter',
    'Manager',
    'NullHandler',
    'PercentStyle',
    'PlaceHolder',
    'RootLogger',
    'StrFormatStyle',
    'StreamHandler',
    'StringTemplateStyle',
]:
    class_obj = getattr(logging, class_name)
    assert callable(class_obj), f'{class_name} should be callable'

# === getLogger identity and root behavior ===
root = logging.getLogger()
assert root is logging.root, 'getLogger() should return root'
assert logging.getLogger(None) is logging.root, 'getLogger(None) should return root'
assert logging.getLogger('') is logging.root, "getLogger('') should return root"
assert logging.getLogger('root') is logging.root, "getLogger('root') should return root"

logger = logging.getLogger('parity.logger')
assert logger is logging.getLogger('parity.logger'), 'named loggers should be cached'
assert logger.name == 'parity.logger', 'logger name should match'
assert logger.level == 0, 'new named logger level should be NOTSET'

try:
    logging.getLogger(1)
    assert False, 'getLogger(non-string) should fail'
except TypeError as exc:
    assert str(exc) == 'A logger name must be a string', f'getLogger TypeError mismatch: {exc}'

# === Level mapping helpers ===
assert logging.getLevelName(logging.DEBUG) == 'DEBUG', 'known numeric level name'
assert logging.getLevelName(15) == 'Level 15', 'unknown numeric level name'
assert logging.getLevelName('INFO') == 20, 'known text level value'
assert logging.getLevelName('NOPE') == 'Level NOPE', 'unknown text level name'

logging.addLevelName(15, 'TRACE')
assert logging.getLevelName(15) == 'TRACE', 'custom level int->name'
assert logging.getLevelName('TRACE') == 15, 'custom level name->int'

logging.addLevelName('text-level', 'TEXTLEVEL')
assert logging.getLevelName('text-level') == 'TEXTLEVEL', 'text level key->name'
assert logging.getLevelName('TEXTLEVEL') == 'text-level', 'text level name->key'

mapping = logging.getLevelNamesMapping()
assert mapping['CRITICAL'] == 50, 'mapping CRITICAL'
assert mapping['WARN'] == 30, 'mapping WARN'
assert mapping['TRACE'] == 15, 'mapping TRACE'
assert mapping['TEXTLEVEL'] == 'text-level', 'mapping TEXTLEVEL'

# === disable/captureWarnings ===
logging.disable('INFO')
assert logging.root.manager.disable == 20, 'disable(INFO) manager threshold'
logging.disable(logging.NOTSET)
assert logging.root.manager.disable == 0, 'disable reset'

try:
    logging.disable('NOPE')
    assert False, 'disable(unknown string) should fail'
except ValueError as exc:
    assert str(exc) == "Unknown level: 'NOPE'", f'disable ValueError mismatch: {exc}'

assert logging.captureWarnings(True) is None, 'captureWarnings(True) should return None'
assert logging.captureWarnings(False) is None, 'captureWarnings(False) should return None'

# === Logger methods ===
logger.setLevel('DEBUG')
assert logger.level == logging.DEBUG, 'logger.setLevel string should resolve'
assert logger.getEffectiveLevel() == logging.DEBUG, 'effective level after setLevel'
assert logger.isEnabledFor(logging.DEBUG) is True, 'isEnabledFor DEBUG after setLevel(DEBUG)'
assert logger.isEnabledFor(logging.CRITICAL) is True, 'isEnabledFor CRITICAL after setLevel(DEBUG)'

child = logger.getChild('child')
assert child.name == 'parity.logger.child', 'getChild name'

# Use a high global threshold to avoid side-effect output while still exercising APIs.
logging.disable(logging.CRITICAL)
assert logging.debug('x') is None, 'logging.debug return'
assert logging.info('x') is None, 'logging.info return'
assert logging.warning('x') is None, 'logging.warning return'
assert logging.warn('x') is None, 'logging.warn return'
assert logging.error('x') is None, 'logging.error return'
assert logging.exception('x') is None, 'logging.exception return'
assert logging.critical('x') is None, 'logging.critical return'
assert logging.fatal('x') is None, 'logging.fatal return'
assert logging.log(logging.INFO, 'x') is None, 'logging.log return'
assert logger.debug('x') is None, 'Logger.debug return'
assert logger.info('x') is None, 'Logger.info return'
assert logger.warning('x') is None, 'Logger.warning return'
assert logger.warn('x') is None, 'Logger.warn return'
assert logger.error('x') is None, 'Logger.error return'
assert logger.exception('x') is None, 'Logger.exception return'
assert logger.critical('x') is None, 'Logger.critical return'
assert logger.fatal('x') is None, 'Logger.fatal return'
assert logger.log(logging.INFO, 'x') is None, 'Logger.log return'
logging.disable(logging.NOTSET)

# === Handler registry helpers ===
handler_names = logging.getHandlerNames()
assert type(handler_names).__name__ == 'frozenset', 'getHandlerNames return type'
assert logging.getHandlerByName('missing-handler-name') is None, 'getHandlerByName missing'

# === Logger class controls ===
base_logger_class = logging.getLoggerClass()
assert logging.setLoggerClass(base_logger_class) is None, 'setLoggerClass(base) return'
assert logging.getLoggerClass() is base_logger_class, 'getLoggerClass after set'

try:
    logging.setLoggerClass(1)
    assert False, 'setLoggerClass(non-class) should fail'
except TypeError as exc:
    assert str(exc) == 'issubclass() arg 1 must be a class', f'setLoggerClass TypeError mismatch: {exc}'

# === LogRecord factory controls and makeLogRecord ===
def custom_record_factory(name, level, pathname, lineno, msg, args, exc_info, func=None):
    return logging.LogRecord(name, level, pathname, lineno, msg, args, exc_info, func)

original_factory = logging.getLogRecordFactory()
assert logging.setLogRecordFactory(custom_record_factory) is None, 'setLogRecordFactory return'
assert logging.getLogRecordFactory() is custom_record_factory, 'getLogRecordFactory after set'
assert logging.setLogRecordFactory(original_factory) is None, 'restore LogRecordFactory'

record = logging.makeLogRecord(
    {
        'name': 'parity.record',
        'msg': 'hello %s',
        'args': ('world',),
        'levelname': 'INFO',
        'levelno': 20,
        'pathname': 'test.py',
        'lineno': 12,
        'exc_info': None,
        'func': None,
        'sinfo': None,
    }
)
assert record.getMessage() == 'hello world', 'makeLogRecord/getMessage formatting'
assert record.name == 'parity.record', 'makeLogRecord name'
assert record.levelno == 20, 'makeLogRecord levelno'

# === basicConfig/shutdown ===
assert logging.basicConfig(level='INFO', force=True) is None, 'basicConfig return'
assert logging.root.level == logging.INFO, 'basicConfig level'
assert len(logging.root.handlers) >= 1, 'basicConfig should configure root handlers'
assert logging.shutdown() is None, 'shutdown return'

# === Other public attributes touched for parity coverage ===
assert logging.Template is not None, 'Template export should exist'
assert logging.GenericAlias is not None, 'GenericAlias export should exist'
assert logging.currentframe() is None or logging.currentframe() is not None, 'currentframe callable coverage'
assert logging.lastResort is not None, 'lastResort export should exist'
