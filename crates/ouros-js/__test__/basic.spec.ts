import test from 'ava'

import { Sandbox, OurosSyntaxError } from '../wrapper'

// =============================================================================
// Constructor tests
// =============================================================================

test('Sandbox constructor with default options', (t) => {
  const m = new Sandbox('1 + 2')
  t.is(m.scriptName, 'main.py')
  t.deepEqual(m.inputs, [])
  t.deepEqual(m.externalFunctions, [])
})

test('Sandbox constructor with custom script name', (t) => {
  const m = new Sandbox('1 + 2', { scriptName: 'test.py' })
  t.is(m.scriptName, 'test.py')
})

test('Sandbox constructor with inputs', (t) => {
  const m = new Sandbox('x + y', { inputs: ['x', 'y'] })
  t.deepEqual(m.inputs, ['x', 'y'])
})

test('Sandbox constructor with external functions', (t) => {
  const m = new Sandbox('foo()', { externalFunctions: ['foo'] })
  t.deepEqual(m.externalFunctions, ['foo'])
})

test('Sandbox constructor with syntax error', (t) => {
  const error = t.throws(() => new Sandbox('def'), { instanceOf: OurosSyntaxError })
  t.true(error?.message.includes('SyntaxError'))
})

// =============================================================================
// repr() tests
// =============================================================================

test('Sandbox repr() no inputs', (t) => {
  const m = new Sandbox('1 + 1')
  const repr = m.repr()
  t.true(repr.includes('Sandbox'))
  t.true(repr.includes('main.py'))
})

test('Sandbox repr() with inputs', (t) => {
  const m = new Sandbox('x', { inputs: ['x', 'y'] })
  const repr = m.repr()
  t.true(repr.includes('Sandbox'))
  t.true(repr.includes('inputs'))
})

test('Sandbox repr() with external functions', (t) => {
  const m = new Sandbox('foo()', { externalFunctions: ['foo'] })
  const repr = m.repr()
  t.true(repr.includes('externalFunctions'))
})

test('Sandbox repr() with inputs and external functions', (t) => {
  const m = new Sandbox('foo(x)', { inputs: ['x'], externalFunctions: ['foo'] })
  const repr = m.repr()
  t.true(repr.includes('inputs'))
  t.true(repr.includes('externalFunctions'))
})

// =============================================================================
// Simple expression tests
// =============================================================================

test('simple expression', (t) => {
  const m = new Sandbox('1 + 2')
  t.is(m.run(), 3)
})

test('arithmetic', (t) => {
  const m = new Sandbox('10 * 5 - 3')
  t.is(m.run(), 47)
})

test('string concatenation', (t) => {
  const m = new Sandbox('"hello" + " " + "world"')
  t.is(m.run(), 'hello world')
})

// =============================================================================
// Multiple runs tests
// =============================================================================

test('multiple runs same instance', (t) => {
  const m = new Sandbox('x * 2', { inputs: ['x'] })
  t.is(m.run({ inputs: { x: 5 } }), 10)
  t.is(m.run({ inputs: { x: 10 } }), 20)
  t.is(m.run({ inputs: { x: -3 } }), -6)
})

test('run multiple times no inputs', (t) => {
  const m = new Sandbox('1 + 2')
  t.is(m.run(), 3)
  t.is(m.run(), 3)
  t.is(m.run(), 3)
})

// =============================================================================
// Multiline code tests
// =============================================================================

test('multiline code', (t) => {
  const code = `
x = 1
y = 2
x + y
`
  const m = new Sandbox(code)
  t.is(m.run(), 3)
})

test('function definition and call', (t) => {
  const code = `
def add(a, b):
    return a + b

add(3, 4)
`
  const m = new Sandbox(code)
  t.is(m.run(), 7)
})
