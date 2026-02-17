//! Tests for the `HeapStats` snapshot feature.
//!
//! Verifies that `ReplSession::heap_stats()` returns accurate, deterministic
//! snapshots of heap state including live object counts, free slot counts,
//! and per-type breakdowns.

use ouros::{HeapStats, NoPrint, ReplSession};

// =============================================================================
// 1. Fresh Session Stats
// =============================================================================

/// A fresh REPL session with no executed code should have zero or minimal live objects.
#[test]
fn fresh_session_has_minimal_live_objects() {
    let session = ReplSession::new(vec![], "<test>");
    let stats = session.heap_stats();
    assert_eq!(
        stats.live_objects,
        0,
        "fresh session should have 0 live objects, got {lo}",
        lo = stats.live_objects
    );
    assert_eq!(
        stats.free_slots,
        0,
        "fresh session should have 0 free slots, got {fs}",
        fs = stats.free_slots
    );
    assert_eq!(
        stats.total_slots,
        0,
        "fresh session total_slots should be 0, got {ts}",
        ts = stats.total_slots
    );
}

// =============================================================================
// 2. Object Counting After Code Execution
// =============================================================================

/// Running `x = [1, 2, 3]` should increase the live object count (at least a List).
#[test]
fn executing_list_increases_live_objects() {
    let mut session = ReplSession::new(vec![], "<test>");
    let before = session.heap_stats();
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let after = session.heap_stats();
    assert!(
        after.live_objects > before.live_objects,
        "live_objects should increase after allocating a list: before={b}, after={a}",
        b = before.live_objects,
        a = after.live_objects
    );
}

/// The objects_by_type map should contain "List" after creating a list.
#[test]
fn objects_by_type_contains_list() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let stats = session.heap_stats();
    assert!(
        stats.objects_by_type.contains_key("List"),
        "objects_by_type should contain 'List' after creating a list, got: {obt:?}",
        obt = stats.objects_by_type
    );
    assert!(
        *stats.objects_by_type.get("List").unwrap() >= 1,
        "should have at least 1 List object"
    );
}

/// Creating a dict should show "Dict" in the type breakdown.
#[test]
fn objects_by_type_contains_dict() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("d = {'a': 1, 'b': 2}", &mut NoPrint).unwrap();
    let stats = session.heap_stats();
    assert!(
        stats.objects_by_type.contains_key("Dict"),
        "objects_by_type should contain 'Dict' after creating a dict, got: {obt:?}",
        obt = stats.objects_by_type
    );
}

// =============================================================================
// 3. Determinism
// =============================================================================

/// Calling heap_stats() twice without mutations should return identical results.
#[test]
fn stats_are_deterministic() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let stats1 = session.heap_stats();
    let stats2 = session.heap_stats();
    assert_eq!(
        stats1, stats2,
        "calling heap_stats() twice should return identical results"
    );
}

// =============================================================================
// 4. Total Slots Invariant
// =============================================================================

/// total_slots should always equal live_objects + free_slots.
#[test]
fn total_slots_invariant() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let stats = session.heap_stats();
    assert_eq!(
        stats.total_slots,
        stats.live_objects + stats.free_slots,
        "total_slots ({ts}) should equal live_objects ({lo}) + free_slots ({fs})",
        ts = stats.total_slots,
        lo = stats.live_objects,
        fs = stats.free_slots
    );
}

// =============================================================================
// 5. Interned Strings
// =============================================================================

/// After running code with string literals, interned_strings should be non-negative.
#[test]
fn interned_strings_count() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("s = 'hello'", &mut NoPrint).unwrap();
    let stats = session.heap_stats();
    // The interner always has at least the base set of pre-interned strings
    assert!(
        stats.interned_strings >= 1,
        "interned_strings should be >= 1 after using a string literal, got {is}",
        is = stats.interned_strings
    );
}

// =============================================================================
// 6. Tracker Stats (NoLimitTracker returns None)
// =============================================================================

/// A default ReplSession uses NoLimitTracker, so tracker stats should be None.
#[test]
fn no_limit_tracker_stats_are_none() {
    let session = ReplSession::new(vec![], "<test>");
    let stats = session.heap_stats();
    assert_eq!(
        stats.tracker_allocations, None,
        "tracker_allocations should be None for NoLimitTracker"
    );
    assert_eq!(
        stats.tracker_memory_bytes, None,
        "tracker_memory_bytes should be None for NoLimitTracker"
    );
}

// =============================================================================
// 7. Clone and Debug
// =============================================================================

/// HeapStats should implement Clone and Debug, and be re-exported from the crate root.
#[test]
fn heap_stats_clone_and_debug() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("x = 42", &mut NoPrint).unwrap();
    let stats: HeapStats = session.heap_stats();
    let cloned = stats.clone();
    assert_eq!(stats, cloned, "cloned HeapStats should equal original");
    let debug = format!("{stats:?}");
    assert!(
        debug.contains("HeapStats"),
        "Debug output should contain 'HeapStats', got: {debug}",
    );
}
