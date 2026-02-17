//! Tests for the `HeapDiff` struct and `HeapStats::diff()` method.
//!
//! Verifies that heap diffs correctly capture changes between two `HeapStats`
//! snapshots, including per-type deltas, new/removed types, and tracker stats.

use std::collections::BTreeMap;

use ouros::{HeapDiff, HeapStats, NoPrint, ReplSession};

// =============================================================================
// 1. Identical Snapshots Produce Empty Diff
// =============================================================================

/// Diffing two identical `HeapStats` should produce a diff where `is_empty()` is true
/// and all deltas are zero.
#[test]
fn diff_of_identical_stats_is_empty() {
    let mut session = ReplSession::new(vec![], "<test>");
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let stats = session.heap_stats();
    let diff = stats.diff(&stats);
    assert!(
        diff.is_empty(),
        "diff of identical stats should be empty, got: {diff:?}"
    );
    assert_eq!(diff.live_objects_delta, 0);
    assert_eq!(diff.free_slots_delta, 0);
    assert_eq!(diff.total_slots_delta, 0);
    assert_eq!(diff.interned_strings_delta, 0);
    assert!(diff.new_types.is_empty(), "no new types expected");
    assert!(diff.removed_types.is_empty(), "no removed types expected");
}

// =============================================================================
// 2. Diff Direction: Before -> After Shows Growth as Positive
// =============================================================================

/// When allocating objects, `before.diff(after)` should show positive live_objects_delta.
#[test]
fn diff_direction_positive_for_growth() {
    let mut session = ReplSession::new(vec![], "<test>");
    let before = session.heap_stats();
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let after = session.heap_stats();
    let diff = before.diff(&after);
    assert!(
        diff.live_objects_delta > 0,
        "allocating a list should show positive live_objects_delta, got {}",
        diff.live_objects_delta
    );
    assert!(
        diff.total_slots_delta > 0,
        "total_slots_delta should also be positive, got {}",
        diff.total_slots_delta
    );
}

// =============================================================================
// 3. Per-Type Deltas
// =============================================================================

/// Creating a list should show a positive delta for "List" in objects_by_type_delta.
#[test]
fn per_type_delta_for_list() {
    let mut session = ReplSession::new(vec![], "<test>");
    let before = session.heap_stats();
    session.execute("x = [1, 2, 3]", &mut NoPrint).unwrap();
    let after = session.heap_stats();
    let diff = before.diff(&after);
    let list_delta = diff.objects_by_type_delta.get("List").copied().unwrap_or(0);
    assert!(
        list_delta > 0,
        "List type delta should be positive after creating a list, got {list_delta}",
    );
}

// =============================================================================
// 4. New Types Detection
// =============================================================================

/// Types present in "after" but not in "before" should appear in `new_types`.
#[test]
fn new_types_detected() {
    let before = HeapStats {
        live_objects: 0,
        free_slots: 0,
        total_slots: 0,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let mut after_types = BTreeMap::new();
    after_types.insert("List", 2_usize);
    after_types.insert("Dict", 1_usize);
    let after = HeapStats {
        live_objects: 3,
        free_slots: 0,
        total_slots: 3,
        objects_by_type: after_types,
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    assert!(diff.new_types.contains(&"List"), "List should be a new type");
    assert!(diff.new_types.contains(&"Dict"), "Dict should be a new type");
    assert!(diff.removed_types.is_empty(), "no types should be removed");
}

// =============================================================================
// 5. Removed Types Detection
// =============================================================================

/// Types present in "before" but not in "after" should appear in `removed_types`.
#[test]
fn removed_types_detected() {
    let mut before_types = BTreeMap::new();
    before_types.insert("List", 2_usize);
    before_types.insert("Str", 1_usize);
    let before = HeapStats {
        live_objects: 3,
        free_slots: 0,
        total_slots: 3,
        objects_by_type: before_types,
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let after = HeapStats {
        live_objects: 0,
        free_slots: 3,
        total_slots: 3,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    assert!(diff.removed_types.contains(&"List"), "List should be removed");
    assert!(diff.removed_types.contains(&"Str"), "Str should be removed");
    assert!(diff.new_types.is_empty(), "no new types expected");
}

// =============================================================================
// 6. Tracker Stats Diff
// =============================================================================

/// When both snapshots have tracker stats, the diff should compute the delta.
#[test]
fn tracker_stats_delta_computed() {
    let before = HeapStats {
        live_objects: 5,
        free_slots: 0,
        total_slots: 5,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: Some(10),
        tracker_memory_bytes: Some(1024),
    };
    let after = HeapStats {
        live_objects: 8,
        free_slots: 0,
        total_slots: 8,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: Some(15),
        tracker_memory_bytes: Some(2048),
    };
    let diff = before.diff(&after);
    assert_eq!(diff.tracker_allocations_delta, Some(5), "tracker allocations delta");
    assert_eq!(
        diff.tracker_memory_bytes_delta,
        Some(1024),
        "tracker memory bytes delta"
    );
}

/// When one snapshot has tracker stats and the other does not, the delta should be None.
#[test]
fn tracker_stats_none_when_mixed() {
    let before = HeapStats {
        live_objects: 5,
        free_slots: 0,
        total_slots: 5,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: Some(10),
        tracker_memory_bytes: Some(1024),
    };
    let after = HeapStats {
        live_objects: 8,
        free_slots: 0,
        total_slots: 8,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    assert_eq!(diff.tracker_allocations_delta, None, "mixed tracker should be None");
    assert_eq!(diff.tracker_memory_bytes_delta, None, "mixed tracker should be None");
}

// =============================================================================
// 7. Negative Deltas (Shrinkage)
// =============================================================================

/// Diffing from a larger to a smaller state should show negative deltas.
#[test]
fn negative_deltas_for_shrinkage() {
    let mut before_types = BTreeMap::new();
    before_types.insert("List", 5_usize);
    let before = HeapStats {
        live_objects: 10,
        free_slots: 2,
        total_slots: 12,
        objects_by_type: before_types,
        interned_strings: 3,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let mut after_types = BTreeMap::new();
    after_types.insert("List", 2_usize);
    let after = HeapStats {
        live_objects: 5,
        free_slots: 7,
        total_slots: 12,
        objects_by_type: after_types,
        interned_strings: 3,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    assert_eq!(diff.live_objects_delta, -5, "should show -5 live objects");
    assert_eq!(diff.free_slots_delta, 5, "should show +5 free slots");
    assert_eq!(diff.total_slots_delta, 0, "total slots unchanged");
    let list_delta = diff.objects_by_type_delta.get("List").copied().unwrap_or(0);
    assert_eq!(list_delta, -3, "List delta should be -3");
}

// =============================================================================
// 8. is_empty False for Non-Zero Diff
// =============================================================================

/// A diff with any non-zero delta should not be empty.
#[test]
fn is_empty_false_for_nonzero_diff() {
    let before = HeapStats {
        live_objects: 0,
        free_slots: 0,
        total_slots: 0,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let after = HeapStats {
        live_objects: 1,
        free_slots: 0,
        total_slots: 1,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    assert!(!diff.is_empty(), "diff with live_objects change should not be empty");
}

// =============================================================================
// 9. Display Output
// =============================================================================

/// The Display impl should produce human-readable output showing deltas.
#[test]
fn display_output_is_reasonable() {
    let mut before_types = BTreeMap::new();
    before_types.insert("List", 1_usize);
    let before = HeapStats {
        live_objects: 2,
        free_slots: 0,
        total_slots: 2,
        objects_by_type: before_types,
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let mut after_types = BTreeMap::new();
    after_types.insert("List", 2_usize);
    after_types.insert("Dict", 1_usize);
    let after = HeapStats {
        live_objects: 5,
        free_slots: 0,
        total_slots: 5,
        objects_by_type: after_types,
        interned_strings: 1,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    let display = format!("{diff}");
    assert!(
        display.contains("HeapDiff"),
        "display should contain 'HeapDiff', got: {display}"
    );
    assert!(
        display.contains("+3 live objects"),
        "display should show +3 live objects, got: {display}"
    );
    assert!(
        display.contains("Dict"),
        "display should mention new type Dict, got: {display}"
    );
}

/// An empty diff should display a simple "no changes" message.
#[test]
fn display_empty_diff() {
    let stats = HeapStats {
        live_objects: 1,
        free_slots: 0,
        total_slots: 1,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = stats.diff(&stats);
    let display = format!("{diff}");
    assert!(
        display.contains("no changes"),
        "empty diff should say 'no changes', got: {display}"
    );
}

// =============================================================================
// 10. Per-Type Delta Includes All Types From Both Snapshots
// =============================================================================

/// Types that exist in only one snapshot should still appear in objects_by_type_delta.
#[test]
fn per_type_delta_union_of_both_snapshots() {
    let mut before_types = BTreeMap::new();
    before_types.insert("Str", 3_usize);
    before_types.insert("List", 1_usize);
    let before = HeapStats {
        live_objects: 4,
        free_slots: 0,
        total_slots: 4,
        objects_by_type: before_types,
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let mut after_types = BTreeMap::new();
    after_types.insert("List", 2_usize);
    after_types.insert("Dict", 1_usize);
    let after = HeapStats {
        live_objects: 3,
        free_slots: 1,
        total_slots: 4,
        objects_by_type: after_types,
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff = before.diff(&after);
    // Str was in before (3) but not in after (0) => delta = -3
    assert_eq!(diff.objects_by_type_delta.get("Str").copied(), Some(-3), "Str delta");
    // List was in before (1) and after (2) => delta = +1
    assert_eq!(diff.objects_by_type_delta.get("List").copied(), Some(1), "List delta");
    // Dict was not in before (0) but in after (1) => delta = +1
    assert_eq!(diff.objects_by_type_delta.get("Dict").copied(), Some(1), "Dict delta");
}

// =============================================================================
// 11. Clone and Debug on HeapDiff
// =============================================================================

/// HeapDiff should implement Clone, Debug, and PartialEq.
#[test]
fn heap_diff_clone_and_debug() {
    let stats = HeapStats {
        live_objects: 1,
        free_slots: 0,
        total_slots: 1,
        objects_by_type: BTreeMap::new(),
        interned_strings: 0,
        tracker_allocations: None,
        tracker_memory_bytes: None,
    };
    let diff: HeapDiff = stats.diff(&stats);
    let cloned = diff.clone();
    assert_eq!(diff, cloned, "cloned HeapDiff should equal original");
    let debug = format!("{diff:?}");
    assert!(
        debug.contains("HeapDiff"),
        "Debug output should contain 'HeapDiff', got: {debug}"
    );
}

// =============================================================================
// 12. Integration: Before and After REPL Execution
// =============================================================================

/// Running code that creates multiple types should produce a meaningful diff.
#[test]
fn integration_repl_diff() {
    let mut session = ReplSession::new(vec![], "<test>");
    let before = session.heap_stats();
    session.execute("x = [1, 2, 3]\nd = {'a': 1}", &mut NoPrint).unwrap();
    let after = session.heap_stats();
    let diff = before.diff(&after);
    assert!(!diff.is_empty(), "diff after executing code should not be empty");
    assert!(
        diff.live_objects_delta > 0,
        "should have more live objects after execution"
    );
    // Both List and Dict should be new types or have positive deltas
    let list_delta = diff.objects_by_type_delta.get("List").copied().unwrap_or(0);
    let dict_delta = diff.objects_by_type_delta.get("Dict").copied().unwrap_or(0);
    assert!(list_delta > 0, "should have positive List delta");
    assert!(dict_delta > 0, "should have positive Dict delta");
}
