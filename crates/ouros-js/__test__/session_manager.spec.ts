import test from 'ava'
import { tmpdir } from 'os'
import { mkdtempSync, rmSync } from 'fs'
import { join } from 'path'

import { SessionManager, Session } from '../session'

// =============================================================================
// Session lifecycle tests
// =============================================================================

test('SessionManager constructor creates default session', (t) => {
  const mgr = new SessionManager()
  const sessions = mgr.listSessions()
  t.is(sessions.length, 1)
  t.is(sessions[0].id, 'default')
})

test('SessionManager constructor with custom script name', (t) => {
  const mgr = new SessionManager({ scriptName: 'test.py' })
  const sessions = mgr.listSessions()
  t.is(sessions.length, 1)
})

test('createSession and listSessions', (t) => {
  const mgr = new SessionManager()
  const session = mgr.createSession('test-session')
  t.true(session instanceof Session)
  t.is(session.id, 'test-session')
  const sessions = mgr.listSessions()
  t.is(sessions.length, 2)
})

test('createSession duplicate throws', (t) => {
  const mgr = new SessionManager()
  mgr.createSession('dup')
  t.throws(() => mgr.createSession('dup'), { message: /already exists/ })
})

test('destroySession removes session', (t) => {
  const mgr = new SessionManager()
  mgr.createSession('to-destroy')
  t.is(mgr.listSessions().length, 2)
  mgr.destroySession('to-destroy')
  t.is(mgr.listSessions().length, 1)
})

test('destroySession default throws', (t) => {
  const mgr = new SessionManager()
  t.throws(() => mgr.destroySession('default'), { message: /cannot destroy/ })
})

test('destroySession nonexistent throws', (t) => {
  const mgr = new SessionManager()
  t.throws(() => mgr.destroySession('nope'), { message: /not found/ })
})

// =============================================================================
// Execute and variable tests
// =============================================================================

test('Session.execute runs Python code', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  const result = session.execute('x = 42')
  t.is(result.stdout, '')
  t.true(result.isComplete)
})

test('Session.execute captures stdout', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  const result = session.execute('print("hello")')
  t.is(result.stdout, 'hello\n')
})

test('Session.execute returns result value', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  const result = session.execute('1 + 2')
  t.true(result.isComplete)
  t.is(result.result, 3)
})

test('Session.listVariables after execute', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = 42')
  session.execute('name = "hello"')
  const vars = session.listVariables()
  t.is(vars.length, 2)
  const names = vars.map((v) => v.name).sort()
  t.deepEqual(names, ['name', 'x'])
})

test('Session.getVariable returns value', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = 42')
  const val = session.getVariable('x')
  t.is(val.jsonValue, 42)
  t.is(val.repr, '42')
})

test('Session.getVariable nonexistent throws', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  t.throws(() => session.getVariable('nope'), { message: /not found/ })
})

test('Session.setVariable creates new variable', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.setVariable('y', '[1, 2, 3]')
  const val = session.getVariable('y')
  t.deepEqual(val.jsonValue, [1, 2, 3])
})

test('Session.deleteVariable removes variable', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('z = 99')
  const deleted = session.deleteVariable('z')
  t.true(deleted)
  t.throws(() => session.getVariable('z'), { message: /not found/ })
})

test('Session.deleteVariable nonexistent returns false', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  const deleted = session.deleteVariable('nope')
  t.false(deleted)
})

test('Session.evalVariable evaluates without side effects', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = 10')
  const result = session.evalVariable('x * 2')
  t.is(result.value.jsonValue, 20)
  // x should still be 10
  const val = session.getVariable('x')
  t.is(val.jsonValue, 10)
})

// =============================================================================
// Fork tests
// =============================================================================

test('Session.fork creates independent copy', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = 42')
  const forked = session.fork('forked')
  t.is(forked.id, 'forked')
  // Forked session has same variable
  t.is(forked.getVariable('x').jsonValue, 42)
  // Modify original -- forked should not change
  session.execute('x = 100')
  t.is(session.getVariable('x').jsonValue, 100)
  t.is(forked.getVariable('x').jsonValue, 42)
})

// =============================================================================
// Transfer variable tests
// =============================================================================

test('transferVariable moves value between sessions', (t) => {
  const mgr = new SessionManager()
  const s1 = mgr.createSession('s1')
  const s2 = mgr.createSession('s2')
  s1.execute('data = [1, 2, 3]')
  mgr.transferVariable('s1', 's2', 'data')
  const val = s2.getVariable('data')
  t.deepEqual(val.jsonValue, [1, 2, 3])
})

test('transferVariable with rename', (t) => {
  const mgr = new SessionManager()
  const s1 = mgr.createSession('a')
  const s2 = mgr.createSession('b')
  s1.execute('x = 42')
  mgr.transferVariable('a', 'b', 'x', 'y')
  const val = s2.getVariable('y')
  t.is(val.jsonValue, 42)
})

// =============================================================================
// Rewind / history tests
// =============================================================================

test('Session.rewind restores previous state', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = 1')
  session.execute('x = 2')
  session.execute('x = 3')
  t.is(session.getVariable('x').jsonValue, 3)
  const result = session.rewind(1)
  t.is(result.stepsRewound, 1)
  t.is(session.getVariable('x').jsonValue, 2)
})

test('Session.history returns depth info', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  const h0 = session.history()
  t.is(h0.current, 0)
  t.true(h0.max > 0)
  session.execute('x = 1')
  const h1 = session.history()
  t.is(h1.current, 1)
})

test('Session.setHistoryDepth changes max depth', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('a = 1')
  session.execute('b = 2')
  session.execute('c = 3')
  const trimmed = session.setHistoryDepth(2)
  t.is(trimmed, 1) // trimmed 1 entry from 3 -> 2
  const h = session.history()
  t.is(h.current, 2)
  t.is(h.max, 2)
})

// =============================================================================
// Heap stats tests
// =============================================================================

test('Session.heapStats returns stats object', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = [1, 2, 3]')
  const stats = session.heapStats()
  t.is(typeof stats.liveObjects, 'number')
  t.is(typeof stats.freeSlots, 'number')
  t.is(typeof stats.totalSlots, 'number')
  t.true(stats.liveObjects > 0)
})

// =============================================================================
// Heap diff tests
// =============================================================================

test('heap snapshot and diff', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  mgr.snapshotHeap('before')
  session.execute('x = [1, 2, 3, 4, 5]')
  mgr.snapshotHeap('after')
  const diff = mgr.diffHeap('before', 'after')
  t.is(typeof diff.heapDiff.liveObjectsDelta, 'number')
  t.true(diff.heapDiff.liveObjectsDelta > 0) // added objects
  t.true(diff.variableDiff.added.includes('x'))
})

// =============================================================================
// Reset tests
// =============================================================================

test('Session.reset clears state', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  session.execute('x = 42')
  t.is(session.listVariables().length, 1)
  session.reset()
  t.is(session.listVariables().length, 0)
})

// =============================================================================
// Save / load tests
// =============================================================================

test('save and load session', (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'ouros-session-test-'))
  try {
    const mgr = new SessionManager({ storageDir: dir })
    const session = mgr.getSession()
    session.execute('x = 42')
    const saveResult = mgr.saveSession()
    t.is(saveResult.name, 'default')
    t.true(saveResult.sizeBytes > 0)

    // Load into new session
    const loadedId = mgr.loadSession('default', 'loaded')
    t.is(loadedId, 'loaded')
    const loadedSession = mgr.getSession('loaded')
    t.is(loadedSession.getVariable('x').jsonValue, 42)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('listSavedSessions', (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'ouros-session-test-'))
  try {
    const mgr = new SessionManager({ storageDir: dir })
    const session = mgr.getSession()
    session.execute('x = 1')
    mgr.saveSession()
    const saved = mgr.listSavedSessions()
    t.is(saved.length, 1)
    t.is(saved[0].name, 'default')
    t.true(saved[0].sizeBytes > 0)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

// =============================================================================
// Cross-session call tests
// =============================================================================

test('callSession pipes result between sessions', (t) => {
  const mgr = new SessionManager()
  const s1 = mgr.createSession('source')
  const s2 = mgr.createSession('target')
  s1.execute('x = 10')
  mgr.callSession('source', 'target', 'x * 2', 'result')
  const val = s2.getVariable('result')
  t.is(val.jsonValue, 20)
})

// =============================================================================
// Error handling tests
// =============================================================================

test('execute with invalid Python throws', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  t.throws(() => session.execute('def'), { message: /.*/ })
})

test('execute with runtime error throws', (t) => {
  const mgr = new SessionManager()
  const session = mgr.getSession()
  t.throws(() => session.execute('1/0'), { message: /.*/ })
})
