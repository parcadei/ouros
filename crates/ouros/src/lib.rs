#![doc = include_str!("../../../README.md")]
#![expect(dead_code, reason = "compat shims keep some currently-unused APIs")]
#![expect(unreachable_patterns, reason = "parity branches stay explicit")]
#![expect(clippy::cast_possible_truncation, reason = "numeric narrowing is checked")]
#![expect(clippy::cast_sign_loss, reason = "sign-changing casts are intentional")]
#![expect(clippy::cast_possible_wrap, reason = "wrap behavior mirrors CPython")]
#![expect(clippy::manual_let_else, reason = "some cleanup paths stay explicit")]
#![expect(clippy::unnecessary_wraps, reason = "dispatch signatures are uniform")]
#![expect(clippy::needless_pass_by_value, reason = "call APIs pass values consistently")]
#![expect(clippy::fn_params_excessive_bools, reason = "bool flags mirror Python kwargs")]
#![expect(clippy::struct_excessive_bools, reason = "state mirrors Python flag fields")]
#![expect(clippy::too_many_arguments, reason = "Python parity requires wide signatures")]
#![expect(clippy::unused_self, reason = "method shapes stay trait-consistent")]
#![expect(clippy::type_complexity, reason = "protocol tuples are intentionally rich")]
#![expect(clippy::trivially_copy_pass_by_ref, reason = "API signatures stay stable")]
#![expect(clippy::assigning_clones, reason = "explicit clone assignment is intentional")]
#![expect(clippy::comparison_chain, reason = "comparison ladders match CPython flow")]
#![expect(clippy::unreadable_literal, reason = "parity constants keep canonical forms")]
#![expect(clippy::approx_constant, reason = "fixtures use CPython decimal literals")]
#![expect(clippy::float_cmp, reason = "parity tests require exact float comparison")]
// first to include defer_drop macro
mod heap;

mod args;
mod asyncio;
mod builtins;
mod bytecode;
pub mod capability;
mod exception_private;
mod exception_public;
mod expressions;
mod fstring;
mod function;
mod intern;
mod io;
mod modules;
mod namespace;
mod object;
mod os;
mod parse;
mod prepare;
mod proxy;
mod py_hash;
mod repl;
mod repl_error;
mod resource;
mod run;
pub mod session_manager;
mod signature;
pub mod tracer;
mod types;
mod value;

#[cfg(feature = "ref-count-return")]
pub use crate::run::RefCountOutput;
pub use crate::{
    exception_private::ExcType,
    exception_public::{CodeLoc, Exception, StackFrame},
    heap::{HeapDiff, HeapStats},
    io::{CollectStringPrint, NoPrint, PrintWriter, StdPrint},
    object::{DictPairs, InvalidInputError, Object},
    os::{OsFunction, dir_stat, file_stat, stat_result, symlink_stat},
    proxy::ProxyId,
    repl::{PendingFutureInfo, ReplProgress, ReplSession, SessionSnapshot},
    repl_error::ReplError,
    resource::{
        DEFAULT_MAX_RECURSION_DEPTH, LimitedTracker, MAX_DATA_RECURSION_DEPTH, NoLimitTracker, ResourceError,
        ResourceLimits, ResourceTracker,
    },
    run::{ExternalResult, FutureSnapshot, OurosFuture, RunProgress, Runner, Snapshot},
    tracer::{
        CoverageTracer, NoopTracer, ProfilingReport, ProfilingTracer, RecordingTracer, StderrTracer, TraceEvent,
        VmTracer,
    },
};
