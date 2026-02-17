//! Implementation of the sorted() builtin function.

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData},
    intern::Interns,
    io::NoPrint,
    resource::ResourceTracker,
    types::{List, OurosIter, list::do_list_sort},
    value::Value,
};

/// Implementation of the sorted() builtin function.
///
/// Returns a new sorted list from the items in an iterable.
///
/// Supports the same keyword arguments as CPython's `sorted()`:
/// - `key`: optional key function
/// - `reverse`: optional bool flag
pub fn builtin_sorted(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
    let (mut positional, kwargs) = args.into_parts();

    let positional_len = positional.len();
    if positional_len != 1 {
        kwargs.drop_with_heap(heap);
        for v in positional {
            v.drop_with_heap(heap);
        }
        return Err(SimpleException::new_msg(
            ExcType::TypeError,
            format!("sorted expected 1 argument, got {positional_len}"),
        )
        .into());
    }

    let iterable = positional.next().unwrap();
    let mut iter = match OurosIter::new(iterable, heap, interns) {
        Ok(iter) => iter,
        Err(err) => {
            kwargs.drop_with_heap(heap);
            return Err(err);
        }
    };
    let items: Vec<_> = match iter.collect(heap, interns) {
        Ok(items) => items,
        Err(err) => {
            iter.drop_with_heap(heap);
            kwargs.drop_with_heap(heap);
            return Err(err);
        }
    };
    iter.drop_with_heap(heap);

    let heap_id = heap.allocate(HeapData::List(List::new(items)))?;
    let sort_args = if kwargs.is_empty() {
        ArgValues::Empty
    } else {
        ArgValues::Kwargs(kwargs)
    };
    let mut no_print = NoPrint;
    if let Err(err) = do_list_sort(heap_id, sort_args, heap, interns, &mut no_print) {
        Value::Ref(heap_id).drop_with_heap(heap);
        return Err(err);
    }

    Ok(Value::Ref(heap_id))
}
