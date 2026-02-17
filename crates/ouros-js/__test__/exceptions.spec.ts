import test from 'ava'

import type { ErrorConstructor } from 'ava'

import { Sandbox, OurosError, OurosSyntaxError, OurosRuntimeError, SandboxTypingError } from '../wrapper'

// Helper for asserting OurosRuntimeError, private constructor requires the awkward cast via any
// but it works fine at runtime
export const isRuntimeError = { instanceOf: OurosRuntimeError as any as ErrorConstructor<OurosRuntimeError> }

// =============================================================================
// OurosRuntimeError tests
// =============================================================================

test('zero division error', (t) => {
  const m = new Sandbox('1 / 0')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'ZeroDivisionError: division by zero')
})

test('value error', (t) => {
  const m = new Sandbox('raise ValueError("bad value")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'ValueError: bad value')
})

test('type error', (t) => {
  const m = new Sandbox("'string' + 1")
  const error = t.throws(() => m.run(), isRuntimeError)
  t.true(error.message.includes('TypeError'))
})

test('index error', (t) => {
  const m = new Sandbox('[1, 2, 3][10]')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'IndexError: list index out of range')
})

test('key error', (t) => {
  const m = new Sandbox('{"a": 1}["b"]')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'KeyError: b')
})

test('attribute error', (t) => {
  const m = new Sandbox('raise AttributeError("no such attr")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'AttributeError: no such attr')
})

test('name error', (t) => {
  const m = new Sandbox('undefined_variable')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, "NameError: name 'undefined_variable' is not defined")
})

test('assertion error', (t) => {
  const m = new Sandbox('assert False')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.true(error.message.includes('AssertionError'))
})

test('assertion error with message', (t) => {
  const m = new Sandbox('assert False, "custom message"')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'AssertionError: custom message')
})

test('runtime error', (t) => {
  const m = new Sandbox('raise RuntimeError("runtime error")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'RuntimeError: runtime error')
})

test('not implemented error', (t) => {
  const m = new Sandbox('raise NotImplementedError("not implemented")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'NotImplementedError: not implemented')
})

// =============================================================================
// OurosSyntaxError tests
// =============================================================================

test('syntax error on init', (t) => {
  const error = t.throws(() => new Sandbox('def'), { instanceOf: OurosSyntaxError })
  t.true(error.message.includes('SyntaxError'))
})

test('syntax error unclosed paren', (t) => {
  const error = t.throws(() => new Sandbox('print(1'), { instanceOf: OurosSyntaxError })
  t.true(error.message.includes('SyntaxError'))
})

test('syntax error invalid syntax', (t) => {
  const error = t.throws(() => new Sandbox('x = = 1'), { instanceOf: OurosSyntaxError })
  t.true(error.message.includes('SyntaxError'))
})

// =============================================================================
// Catching with base class tests
// =============================================================================

test('catch with base class', (t) => {
  const m = new Sandbox('1 / 0')
  try {
    m.run()
    t.fail('Should have thrown')
  } catch (e) {
    t.true(e instanceof OurosError)
  }
})

test('catch syntax error with base class', (t) => {
  try {
    new Sandbox('def')
  } catch (e) {
    t.true(e instanceof OurosError)
  }
})

// =============================================================================
// Exception handling within sandbox tests
// =============================================================================

test('raise caught exception', (t) => {
  const code = `
try:
    1 / 0
except ZeroDivisionError as e:
    result = 'caught'
result
`
  const m = new Sandbox(code)
  t.is(m.run(), 'caught')
})

test('exception in function', (t) => {
  const code = `
def fail():
    raise ValueError('from function')

fail()
`
  const m = new Sandbox(code)
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'ValueError: from function')
})

// =============================================================================
// Display and str methods tests
// =============================================================================

test('display traceback', (t) => {
  const m = new Sandbox('1 / 0')
  const error = t.throws(() => m.run(), isRuntimeError)
  const display = error.display('traceback')
  t.true(display.includes('Traceback (most recent call last):'))
  t.true(display.includes('ZeroDivisionError'))
})

test('display type msg', (t) => {
  const m = new Sandbox('raise ValueError("test message")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.display('type-msg'), 'ValueError: test message')
})

test('runtime display', (t) => {
  const m = new Sandbox('raise ValueError("test message")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.display('msg'), 'test message')
  t.is(error.display('type-msg'), 'ValueError: test message')
  const traceback = error.display('traceback')
  t.true(traceback.includes('Traceback (most recent call last):'))
  t.true(
    traceback.includes("raise ValueError('test message')") || traceback.includes('raise ValueError("test message")'),
  )
  t.true(traceback.includes('ValueError: test message'))
})

test('str returns type msg', (t) => {
  const m = new Sandbox('raise ValueError("test message")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.message, 'ValueError: test message')
})

test('syntax error display', (t) => {
  const error = t.throws(() => new Sandbox('def'), { instanceOf: OurosSyntaxError })
  t.true(error.display().includes('Expected an identifier'))
  t.true(error.display('type-msg').includes('SyntaxError'))
})

// =============================================================================
// Traceback tests
// =============================================================================

test('traceback frames', (t) => {
  const code = `def inner():
    raise ValueError('error')

def outer():
    inner()

outer()
`
  const m = new Sandbox(code)
  const error = t.throws(() => m.run(), isRuntimeError)
  const display = error.display('traceback')

  t.true(display.includes('Traceback (most recent call last):'))
  t.true(display.includes('outer()'))
  t.true(display.includes('inner()'))
  t.true(display.includes('ValueError: error'))
})

// =============================================================================
// OurosError base class tests
// =============================================================================

test('OurosError extends Error', (t) => {
  const err = new OurosError('ValueError', 'test message')
  t.true(err instanceof Error)
  t.true(err instanceof OurosError)
  t.is(err.name, 'OurosError')
})

test('OurosError constructor and properties', (t) => {
  const err = new OurosError('ValueError', 'test message')
  t.deepEqual(err.exception, { typeName: 'ValueError', message: 'test message' })
  t.is(err.message, 'ValueError: test message')
})

test('OurosError display()', (t) => {
  const err = new OurosError('ValueError', 'test message')
  t.is(err.display('msg'), 'test message')
  t.is(err.display('type-msg'), 'ValueError: test message')
})

test('OurosError with empty message', (t) => {
  const err = new OurosError('TypeError', '')
  t.is(err.display('type-msg'), 'TypeError')
})

// =============================================================================
// OurosSyntaxError class tests
// =============================================================================

test('OurosSyntaxError extends OurosError and Error', (t) => {
  const err = new OurosSyntaxError('invalid syntax')
  t.true(err instanceof Error)
  t.true(err instanceof OurosError)
  t.true(err instanceof OurosSyntaxError)
  t.is(err.name, 'OurosSyntaxError')
})

test('OurosSyntaxError constructor and properties', (t) => {
  const err = new OurosSyntaxError('invalid syntax')
  t.deepEqual(err.exception, { typeName: 'SyntaxError', message: 'invalid syntax' })
  t.is(err.message, 'SyntaxError: invalid syntax')
})

test('OurosSyntaxError display()', (t) => {
  const err = new OurosSyntaxError('unexpected token')
  t.is(err.display(), 'unexpected token')
  t.is(err.display('msg'), 'unexpected token')
  t.is(err.display('type-msg'), 'SyntaxError: unexpected token')
})

// =============================================================================
// OurosRuntimeError class tests
// =============================================================================

test('OurosRuntimeError display()', (t) => {
  const m = new Sandbox('1 / 0')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.true(error instanceof OurosError)
  t.true(error instanceof Error)

  t.is(error.message, 'ZeroDivisionError: division by zero')

  const traceback = error.display('traceback')
  t.is(error.display(), traceback)
  t.true(traceback.includes('Traceback (most recent call last):'))

  t.is(error.display('type-msg'), 'ZeroDivisionError: division by zero')
  t.is(error.display('msg'), 'division by zero')
})

test('OurosRuntimeError can be caught with instanceof', (t) => {
  const m = new Sandbox('1 / 0')
  try {
    m.run()
    t.fail('Should have thrown')
  } catch (e) {
    t.true(e instanceof OurosRuntimeError)
    t.true(e instanceof OurosError)
    t.true(e instanceof Error)
  }
})

// =============================================================================
// SandboxTypingError class tests
// =============================================================================

test('SandboxTypingError extends OurosError and Error', (t) => {
  const err = new SandboxTypingError('type mismatch')
  t.true(err instanceof Error)
  t.true(err instanceof OurosError)
  t.true(err instanceof SandboxTypingError)
  t.is(err.name, 'SandboxTypingError')
})

test('SandboxTypingError is thrown on type check failure', (t) => {
  const code = `
x: int = "not an int"
`
  const error = t.throws(() => new Sandbox(code, { typeCheck: true }), { instanceOf: SandboxTypingError })
  t.true(error instanceof OurosError)
  t.true(error instanceof Error)
})

// =============================================================================
// Error catching hierarchy tests
// =============================================================================

test('OurosError catches all Ouros exceptions', (t) => {
  // Syntax error
  try {
    new Sandbox('def')
  } catch (e) {
    t.true(e instanceof OurosError)
  }

  // Runtime error
  try {
    new Sandbox('1 / 0').run()
  } catch (e) {
    t.true(e instanceof OurosError)
  }

  // Type error
  try {
    new Sandbox('x: int = "str"', { typeCheck: true })
  } catch (e) {
    t.true(e instanceof OurosError)
  }
})

test('can distinguish error types with instanceof', (t) => {
  // Test syntax error
  try {
    new Sandbox('def')
  } catch (e) {
    t.true(e instanceof OurosSyntaxError)
    t.false(e instanceof OurosRuntimeError)
    t.false(e instanceof SandboxTypingError)
  }

  // Test runtime error
  try {
    new Sandbox('1 / 0').run()
  } catch (e) {
    t.true(e instanceof OurosRuntimeError)
    t.false(e instanceof OurosSyntaxError)
    t.false(e instanceof SandboxTypingError)
  }

  // Test type error
  try {
    new Sandbox('x: int = "str"', { typeCheck: true })
  } catch (e) {
    t.true(e instanceof SandboxTypingError)
    t.false(e instanceof OurosSyntaxError)
    t.false(e instanceof OurosRuntimeError)
  }
})

// =============================================================================
// Exception info accessors tests
// =============================================================================

test('exception getter returns correct info for runtime error', (t) => {
  const m = new Sandbox('raise ValueError("test")')
  const error = t.throws(() => m.run(), isRuntimeError)
  t.is(error.exception.typeName, 'ValueError')
  t.is(error.exception.message, 'test')
})

test('exception getter returns correct info for syntax error', (t) => {
  const error = t.throws(() => new Sandbox('def'), { instanceOf: OurosSyntaxError })
  t.is(error.exception.typeName, 'SyntaxError')
})

// =============================================================================
// Polymorphic display() tests
// =============================================================================

test('display() works polymorphically on SandboxTypingError', (t) => {
  try {
    new Sandbox('x: int = "str"', { typeCheck: true })
    t.fail('Should have thrown')
  } catch (e) {
    t.true(e instanceof OurosError)
    const msg = (e as OurosError).display('msg')
    t.true(msg.length > 0)
    const typeMsg = (e as OurosError).display('type-msg')
    t.true(typeMsg.startsWith('TypeError:'))
  }
})
