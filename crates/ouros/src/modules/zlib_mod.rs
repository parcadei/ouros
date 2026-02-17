//! Compatibility implementation of Python's `zlib` module.
//!
//! This module provides:
//! - checksum helpers (`crc32`, `adler32`)
//! - one-shot compression/decompression (`compress`, `decompress`)
//! - stateful streaming objects (`compressobj`, `decompressobj`)
//!
//! The implementation is sandbox-safe and does not expose host I/O.

use std::{
    fmt::Write,
    io::{Read, Write as _},
};

use flate2::{
    Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status, read::GzDecoder, write::GzEncoder,
};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};

use crate::{
    args::ArgValues,
    builtins::Builtins,
    exception_private::{ExcType, RunError, RunResult, SimpleException},
    heap::{DropWithHeap, Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings, StringId},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Bytes, Module, PyTrait, Str, Type},
    value::{EitherStr, Value},
};

/// Adler-32 prime modulus.
const ADLER32_MOD: u32 = 65_521;
/// The `zlib.DEFLATED` constant value.
const ZLIB_DEFLATED: i64 = 8;
/// Default output chunk size used for streaming operations.
const STREAM_CHUNK_SIZE: usize = 16_384;

/// `zlib` module functions implemented by Ouros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ZlibFunctions {
    Crc32,
    Adler32,
    Compress,
    Compressobj,
    Decompress,
    Decompressobj,
}

/// Compression stream wrapper mode selected by `wbits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum CompressMode {
    /// RFC1950 zlib stream.
    Zlib,
    /// Raw RFC1951 deflate stream.
    Raw,
    /// RFC1952 gzip stream.
    Gzip,
}

/// Decompression stream wrapper mode selected by `wbits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum DecompressMode {
    /// RFC1950 zlib stream.
    Zlib,
    /// Raw RFC1951 deflate stream.
    Raw,
    /// RFC1952 gzip stream.
    Gzip,
    /// Auto-detect zlib vs gzip based on stream header.
    Auto,
}

/// One replayable operation performed on a `zlib.compressobj`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum CompressAction {
    /// A `compress(data)` call.
    Compress(Vec<u8>),
    /// A `flush(mode)` call.
    Flush(i64),
}

/// Stateful `zlib.compressobj` runtime value.
///
/// The object stores a replayable action log instead of raw compressor state so
/// it can be serialized safely in snapshots.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ZlibCompressObject {
    /// Requested compression level.
    level: i64,
    /// Compression method (`DEFLATED` only).
    method: i64,
    /// Original `wbits` argument.
    wbits: i64,
    /// Compression memory level.
    mem_level: i64,
    /// Compression strategy.
    strategy: i64,
    /// Optional compression dictionary.
    zdict: Option<Vec<u8>>,
    /// Ordered log of calls applied to this object.
    actions: Vec<CompressAction>,
}

/// One replayable operation performed on a `zlib.decompressobj`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum DecompressAction {
    /// A `decompress(data, max_length)` call.
    Decompress { data: Vec<u8>, max_length: i64 },
    /// A `flush(length)` call.
    Flush { length: i64 },
}

/// Stateful `zlib.decompressobj` runtime value.
///
/// Like `ZlibCompressObject`, this stores an action log for snapshot safety.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ZlibDecompressObject {
    /// Original `wbits` argument.
    wbits: i64,
    /// Optional dictionary supplied at construction.
    zdict: Vec<u8>,
    /// Ordered log of calls applied to this object.
    actions: Vec<DecompressAction>,
}

/// Live replay state used while evaluating a `ZlibDecompressObject`.
struct ReplayDecompressState {
    /// Configured decompression mode.
    mode: DecompressMode,
    /// Effective window bits (9..=15).
    window_bits: u8,
    /// Optional dictionary.
    zdict: Vec<u8>,
    /// Active decompressor once mode is resolved.
    engine: Option<Decompress>,
    /// Whether the compressed stream reached `StreamEnd`.
    eof: bool,
    /// Remaining compressed input not yet consumed because output was bounded.
    unconsumed_tail: Vec<u8>,
    /// Bytes appearing after stream end.
    unused_data: Vec<u8>,
}

impl ZlibCompressObject {
    /// Replays this object's action log and returns a fresh compressor plus
    /// whether the stream has already been finalized.
    fn replay(&self) -> RunResult<(Compress, bool)> {
        let (mode, window_bits) = parse_compress_wbits(self.wbits, true)?;
        let mut engine = build_compressor(self.level, mode, window_bits)?;
        let mut finished = false;

        for action in &self.actions {
            match action {
                CompressAction::Compress(data) => {
                    let _ = compress_with_engine(&mut engine, data, FlushCompress::None, "compressing data")?;
                }
                CompressAction::Flush(mode) => {
                    let flush_mode = flush_compress_from_i64(*mode)
                        .ok_or_else(|| zlib_error("Error -2 while flushing: inconsistent stream state"))?;
                    let _ = compress_with_engine(&mut engine, &[], flush_mode, "flushing")?;
                    if matches!(flush_mode, FlushCompress::Finish) {
                        finished = true;
                    }
                }
            }
        }

        Ok((engine, finished))
    }
}

impl PyTrait for ZlibCompressObject {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Object
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {}

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .actions
                .iter()
                .map(|action| match action {
                    CompressAction::Compress(data) => data.len(),
                    CompressAction::Flush(_) => 0,
                })
                .sum::<usize>()
            + self.zdict.as_ref().map_or(0, Vec::len)
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut ahash::AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<zlib.Compress object>")
    }

    fn py_getattr(
        &self,
        _attr_id: StringId,
        _heap: &mut Heap<impl ResourceTracker>,
        _interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        Ok(None)
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr.as_str(interns) {
            "compress" => {
                let data = args.get_one_arg("compress", heap)?;
                let bytes = extract_bytes_like(&data, heap, interns)?;
                data.drop_with_heap(heap);

                let (mut engine, finished) = self.replay()?;
                if finished {
                    return Err(zlib_error("Error -2 while compressing data: inconsistent stream state"));
                }
                let output = compress_with_engine(&mut engine, &bytes, FlushCompress::None, "compressing data")?;
                self.actions.push(CompressAction::Compress(bytes));
                let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
                Ok(Value::Ref(id))
            }
            "flush" => {
                let mode = args.get_zero_one_arg("flush", heap)?;
                let mode = match mode {
                    Some(value) => {
                        let parsed = value.as_int(heap)?;
                        value.drop_with_heap(heap);
                        parsed
                    }
                    None => 4,
                };

                let flush_mode = flush_compress_from_i64(mode)
                    .ok_or_else(|| zlib_error("Error -2 while flushing: inconsistent stream state"))?;
                let (mut engine, finished) = self.replay()?;
                if finished {
                    return Err(zlib_error("Error -2 while flushing: inconsistent stream state"));
                }
                let output = compress_with_engine(&mut engine, &[], flush_mode, "flushing")?;
                self.actions.push(CompressAction::Flush(mode));
                let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
                Ok(Value::Ref(id))
            }
            "copy" => {
                args.check_zero_args("copy", heap)?;
                let (_, finished) = self.replay()?;
                if finished {
                    return Err(zlib_inconsistent_stream_state_value_error());
                }
                let id = heap.allocate(HeapData::ZlibCompress(self.clone()))?;
                Ok(Value::Ref(id))
            }
            name => Err(ExcType::attribute_error("zlib.Compress", name)),
        }
    }
}

impl ZlibDecompressObject {
    /// Replays all previous calls and returns the resulting live decompression
    /// state ready for one additional action.
    fn replay_state(&self) -> RunResult<ReplayDecompressState> {
        let (mode, window_bits) = parse_decompress_wbits(self.wbits, true)?;
        let mut state = ReplayDecompressState {
            mode,
            window_bits,
            zdict: self.zdict.clone(),
            engine: match mode {
                DecompressMode::Auto => None,
                _ => Some(build_decompressor(mode, window_bits)),
            },
            eof: false,
            unconsumed_tail: Vec::new(),
            unused_data: Vec::new(),
        };

        for action in &self.actions {
            match action {
                DecompressAction::Decompress { data, max_length } => {
                    let _ = state.decompress(data, *max_length, false)?;
                }
                DecompressAction::Flush { length } => {
                    let _ = state.decompress(&[], 0, true)?;
                    if *length <= 0 {
                        return Err(
                            SimpleException::new_msg(ExcType::ValueError, "length must be greater than zero").into(),
                        );
                    }
                }
            }
        }
        Ok(state)
    }
}

impl PyTrait for ZlibDecompressObject {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        Type::Object
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, _other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        false
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {}

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.zdict.len()
            + self
                .actions
                .iter()
                .map(|action| match action {
                    DecompressAction::Decompress { data, .. } => data.len(),
                    DecompressAction::Flush { .. } => 0,
                })
                .sum::<usize>()
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut ahash::AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        f.write_str("<zlib.Decompress object>")
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let state = self.replay_state()?;
        match interns.get_str(attr_id) {
            "eof" => Ok(Some(AttrCallResult::Value(Value::Bool(state.eof)))),
            "unused_data" => {
                let id = heap.allocate(HeapData::Bytes(Bytes::new(state.unused_data)))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
            "unconsumed_tail" => {
                let id = heap.allocate(HeapData::Bytes(Bytes::new(state.unconsumed_tail)))?;
                Ok(Some(AttrCallResult::Value(Value::Ref(id))))
            }
            _ => Ok(None),
        }
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        match attr.as_str(interns) {
            "decompress" => {
                let (data, max_length) =
                    args.get_one_two_args_with_keyword("decompress", "max_length", heap, interns)?;
                let data_bytes = extract_bytes_like(&data, heap, interns)?;
                data.drop_with_heap(heap);
                let max_length = match max_length {
                    Some(value) => {
                        let parsed = value.as_int(heap)?;
                        value.drop_with_heap(heap);
                        parsed
                    }
                    None => 0,
                };
                if max_length < 0 {
                    return Err(
                        SimpleException::new_msg(ExcType::ValueError, "max_length must be non-negative").into(),
                    );
                }

                let mut state = self.replay_state()?;
                let output = state.decompress(&data_bytes, max_length, false)?;
                self.actions.push(DecompressAction::Decompress {
                    data: data_bytes,
                    max_length,
                });
                let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
                Ok(Value::Ref(id))
            }
            "flush" => {
                let length = args.get_zero_one_arg("flush", heap)?;
                let length = match length {
                    Some(value) => {
                        let parsed = value.as_int(heap)?;
                        value.drop_with_heap(heap);
                        parsed
                    }
                    None => 16_384,
                };
                if length <= 0 {
                    return Err(
                        SimpleException::new_msg(ExcType::ValueError, "length must be greater than zero").into(),
                    );
                }

                let mut state = self.replay_state()?;
                let output = state.decompress(&[], 0, true)?;
                self.actions.push(DecompressAction::Flush { length });
                let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
                Ok(Value::Ref(id))
            }
            "copy" => {
                args.check_zero_args("copy", heap)?;
                let state = self.replay_state()?;
                if state.eof {
                    return Err(zlib_inconsistent_stream_state_value_error());
                }
                let id = heap.allocate(HeapData::ZlibDecompress(self.clone()))?;
                Ok(Value::Ref(id))
            }
            name => Err(ExcType::attribute_error("zlib.Decompress", name)),
        }
    }
}

impl ReplayDecompressState {
    /// Ensures an engine exists and resolves auto mode using stream header
    /// bytes when needed.
    fn ensure_engine(&mut self, input: &[u8]) {
        if self.engine.is_some() {
            return;
        }
        let mode =
            if matches!(self.mode, DecompressMode::Auto) && input.len() >= 2 && input[0] == 0x1f && input[1] == 0x8b {
                DecompressMode::Gzip
            } else {
                DecompressMode::Zlib
            };
        self.engine = Some(build_decompressor(mode, self.window_bits));
    }

    /// Applies one decompression step with CPython-style `zlib.decompressobj`
    /// semantics.
    fn decompress(&mut self, data: &[u8], max_length: i64, finish_flush: bool) -> RunResult<Vec<u8>> {
        if self.eof {
            self.unused_data.extend_from_slice(data);
            return Ok(Vec::new());
        }

        let mut input = Vec::with_capacity(self.unconsumed_tail.len() + data.len());
        input.extend_from_slice(&self.unconsumed_tail);
        input.extend_from_slice(data);
        self.unconsumed_tail.clear();

        self.ensure_engine(&input);
        let engine = self.engine.as_mut().expect("engine must be initialized");
        let mut output = Vec::new();
        let mut input_offset = 0usize;
        let max_output = if max_length <= 0 {
            usize::MAX
        } else {
            usize::try_from(max_length).unwrap_or(usize::MAX)
        };

        loop {
            if max_output != usize::MAX && output.len() >= max_output {
                self.unconsumed_tail = input[input_offset..].to_vec();
                break;
            }

            let remaining_out = if max_output == usize::MAX {
                STREAM_CHUNK_SIZE
            } else {
                (max_output - output.len()).clamp(1, STREAM_CHUNK_SIZE)
            };
            let mut out_buf = vec![0_u8; remaining_out];
            let before_in = engine.total_in();
            let before_out = engine.total_out();

            let status = match engine.decompress(
                &input[input_offset..],
                &mut out_buf,
                if finish_flush {
                    FlushDecompress::Finish
                } else {
                    FlushDecompress::None
                },
            ) {
                Ok(status) => status,
                Err(err) => {
                    if err.to_string().contains("requires a dictionary") {
                        if self.zdict.is_empty() {
                            return Err(zlib_error("Error 2 while decompressing data"));
                        }
                        return Err(zlib_error("Error -3 while setting zdict: invalid input data"));
                    }
                    let detail = err.message().unwrap_or("invalid input data");
                    return Err(zlib_error(format!("Error -3 while decompressing data: {detail}")));
                }
            };

            let consumed = usize::try_from(engine.total_in().saturating_sub(before_in)).unwrap_or(0);
            let produced = usize::try_from(engine.total_out().saturating_sub(before_out)).unwrap_or(0);

            input_offset = input_offset.saturating_add(consumed);
            output.extend_from_slice(&out_buf[..produced]);

            if matches!(status, Status::StreamEnd) {
                self.eof = true;
                if input_offset < input.len() {
                    self.unused_data.extend_from_slice(&input[input_offset..]);
                }
                break;
            }

            let no_progress = consumed == 0 && produced == 0;
            let input_exhausted = input_offset >= input.len();
            if no_progress || input_exhausted {
                if !input_exhausted {
                    self.unconsumed_tail = input[input_offset..].to_vec();
                }
                break;
            }
        }

        Ok(output)
    }
}

/// Creates the `zlib` module.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Zlib);

    register(&mut module, "crc32", ZlibFunctions::Crc32, heap, interns)?;
    register(&mut module, "adler32", ZlibFunctions::Adler32, heap, interns)?;
    register(&mut module, "compress", ZlibFunctions::Compress, heap, interns)?;
    register(&mut module, "compressobj", ZlibFunctions::Compressobj, heap, interns)?;
    register(&mut module, "decompress", ZlibFunctions::Decompress, heap, interns)?;
    register(
        &mut module,
        "decompressobj",
        ZlibFunctions::Decompressobj,
        heap,
        interns,
    )?;

    module.set_attr_text("Z_NO_COMPRESSION", Value::Int(0), heap, interns)?;
    module.set_attr_text("Z_BEST_SPEED", Value::Int(1), heap, interns)?;
    module.set_attr_text("Z_BEST_COMPRESSION", Value::Int(9), heap, interns)?;
    module.set_attr_text("Z_DEFAULT_COMPRESSION", Value::Int(-1), heap, interns)?;
    module.set_attr_text("DEFLATED", Value::Int(ZLIB_DEFLATED), heap, interns)?;
    module.set_attr_text("DEF_BUF_SIZE", Value::Int(16_384), heap, interns)?;
    module.set_attr_text("DEF_MEM_LEVEL", Value::Int(8), heap, interns)?;
    module.set_attr_text("MAX_WBITS", Value::Int(15), heap, interns)?;
    module.set_attr_text("Z_NO_FLUSH", Value::Int(0), heap, interns)?;
    module.set_attr_text("Z_PARTIAL_FLUSH", Value::Int(1), heap, interns)?;
    module.set_attr_text("Z_SYNC_FLUSH", Value::Int(2), heap, interns)?;
    module.set_attr_text("Z_FULL_FLUSH", Value::Int(3), heap, interns)?;
    module.set_attr_text("Z_FINISH", Value::Int(4), heap, interns)?;
    module.set_attr_text("Z_BLOCK", Value::Int(5), heap, interns)?;
    module.set_attr_text("Z_TREES", Value::Int(6), heap, interns)?;
    module.set_attr_text("Z_FILTERED", Value::Int(1), heap, interns)?;
    module.set_attr_text("Z_HUFFMAN_ONLY", Value::Int(2), heap, interns)?;
    module.set_attr_text("Z_RLE", Value::Int(3), heap, interns)?;
    module.set_attr_text("Z_FIXED", Value::Int(4), heap, interns)?;
    module.set_attr_text("Z_DEFAULT_STRATEGY", Value::Int(0), heap, interns)?;

    let runtime_version = heap.allocate(HeapData::Str(Str::from("1.2.12")))?;
    let zlib_version = heap.allocate(HeapData::Str(Str::from("1.2.12")))?;
    module.set_attr_text("ZLIB_RUNTIME_VERSION", Value::Ref(runtime_version), heap, interns)?;
    module.set_attr_text("ZLIB_VERSION", Value::Ref(zlib_version), heap, interns)?;
    module.set_attr_text(
        "error",
        Value::Builtin(Builtins::ExcType(ExcType::ValueError)),
        heap,
        interns,
    )?;

    heap.allocate(HeapData::Module(module))
}

/// Dispatches calls to `zlib` module functions.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
    function: ZlibFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match function {
        ZlibFunctions::Crc32 => crc32(heap, interns, args)?,
        ZlibFunctions::Adler32 => adler32(heap, interns, args)?,
        ZlibFunctions::Compress => zlib_compress(heap, interns, args)?,
        ZlibFunctions::Compressobj => zlib_compressobj(heap, interns, args)?,
        ZlibFunctions::Decompress => zlib_decompress(heap, interns, args)?,
        ZlibFunctions::Decompressobj => zlib_decompressobj(heap, interns, args)?,
    };
    Ok(AttrCallResult::Value(value))
}

/// Implements `zlib.crc32(data, value=0)`.
fn crc32(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, start) = args.get_one_two_args_with_keyword("zlib.crc32", "value", heap, interns)?;
    let bytes = extract_bytes_like(&data, heap, interns)?;
    data.drop_with_heap(heap);

    let mut crc = 0_u32;
    if let Some(start) = start {
        crc = value_to_u32_mask(&start, heap)?;
        start.drop_with_heap(heap);
    }

    crc = crc32_accumulate(crc, &bytes);
    Ok(Value::Int(i64::from(crc)))
}

/// Implements `zlib.adler32(data, value=1)`.
fn adler32(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (data, start) = args.get_one_two_args_with_keyword("zlib.adler32", "value", heap, interns)?;
    let bytes = extract_bytes_like(&data, heap, interns)?;
    data.drop_with_heap(heap);

    let mut adler = 1_u32;
    if let Some(start) = start {
        adler = value_to_u32_mask(&start, heap)?;
        start.drop_with_heap(heap);
    }

    let mut s1 = adler & 0xffff;
    let mut s2 = adler >> 16;
    for byte in bytes {
        s1 = (s1 + u32::from(byte)) % ADLER32_MOD;
        s2 = (s2 + s1) % ADLER32_MOD;
    }
    adler = (s2 << 16) | s1;
    Ok(Value::Int(i64::from(adler)))
}

/// Implements `zlib.compress(data, /, level=-1, wbits=15)`.
fn zlib_compress(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    let pos_len = pos.len();
    if pos_len == 0 {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("compress", 1, 0));
    }
    if pos_len > 3 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("compress", 3, pos_len));
    }

    let data = pos.next().expect("pos_len > 0");
    let bytes = extract_bytes_like(&data, heap, interns)?;
    data.drop_with_heap(heap);

    let mut level = -1_i64;
    let mut wbits = 15_i64;
    let mut level_from_pos = false;
    let mut wbits_from_pos = false;

    if let Some(value) = pos.next() {
        level = value.as_int(heap)?;
        level_from_pos = true;
        value.drop_with_heap(heap);
    }
    if let Some(value) = pos.next() {
        wbits = value.as_int(heap)?;
        wbits_from_pos = true;
        value.drop_with_heap(heap);
    }

    for (key, value) in kwargs {
        let key_name = kwarg_name_from_value(&key, heap, interns)?;
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "level" => {
                if level_from_pos {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compress", "level"));
                }
                level = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "wbits" => {
                if wbits_from_pos {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compress", "wbits"));
                }
                wbits = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("compress", key_name.as_str()));
            }
        }
    }

    if !(level == -1 || (0..=9).contains(&level)) {
        return Err(zlib_error("Bad compression level"));
    }
    let (mode, window_bits) = parse_compress_wbits(wbits, false).map_err(|_| zlib_error("Bad compression level"))?;
    let output = if matches!(mode, CompressMode::Gzip) {
        let compression = if level == -1 {
            Compression::default()
        } else {
            Compression::new(u32::try_from(level).unwrap_or(0))
        };
        let mut encoder = GzEncoder::new(Vec::new(), compression);
        encoder
            .write_all(&bytes)
            .map_err(|_| zlib_error("Bad compression level"))?;
        encoder.finish().map_err(|_| zlib_error("Bad compression level"))?
    } else {
        let mut engine = build_compressor(level, mode, window_bits).map_err(|_| zlib_error("Bad compression level"))?;
        let mut output = compress_with_engine(&mut engine, &bytes, FlushCompress::None, "compressing data")?;
        let finish = compress_with_engine(&mut engine, &[], FlushCompress::Finish, "compressing data")?;
        output.extend_from_slice(&finish);
        output
    };

    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Implements `zlib.decompress(data, /, wbits=15, bufsize=16384)`.
fn zlib_decompress(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    let pos_len = pos.len();
    if pos_len == 0 {
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_least("decompress", 1, 0));
    }
    if pos_len > 3 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("decompress", 3, pos_len));
    }

    let data = pos.next().expect("pos_len > 0");
    let bytes = extract_bytes_like(&data, heap, interns)?;
    data.drop_with_heap(heap);

    let mut wbits = 15_i64;
    let mut bufsize = 16_384_i64;
    let mut wbits_from_pos = false;
    let mut bufsize_from_pos = false;

    if let Some(value) = pos.next() {
        wbits = value.as_int(heap)?;
        wbits_from_pos = true;
        value.drop_with_heap(heap);
    }
    if let Some(value) = pos.next() {
        bufsize = value.as_int(heap)?;
        bufsize_from_pos = true;
        value.drop_with_heap(heap);
    }

    for (key, value) in kwargs {
        let key_name = kwarg_name_from_value(&key, heap, interns)?;
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "wbits" => {
                if wbits_from_pos {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("decompress", "wbits"));
                }
                wbits = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "bufsize" => {
                if bufsize_from_pos {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("decompress", "bufsize"));
                }
                bufsize = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("decompress", key_name.as_str()));
            }
        }
    }

    if bufsize < 0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "bufsize must be non-negative").into());
    }
    let (mode, window_bits) = parse_decompress_wbits(wbits, false)
        .map_err(|_| zlib_error("Error -2 while preparing to decompress data: inconsistent stream state"))?;
    let mut state = ReplayDecompressState {
        mode,
        window_bits,
        zdict: Vec::new(),
        engine: match mode {
            DecompressMode::Auto => None,
            _ => Some(build_decompressor(mode, window_bits)),
        },
        eof: false,
        unconsumed_tail: Vec::new(),
        unused_data: Vec::new(),
    };
    let output =
        if matches!(mode, DecompressMode::Gzip) || (matches!(mode, DecompressMode::Auto) && is_gzip_header(&bytes)) {
            state.eof = true;
            decompress_gzip_one_shot(&bytes)?
        } else {
            state.decompress(&bytes, 0, true)?
        };
    if !state.eof {
        return Err(zlib_error(
            "Error -5 while decompressing data: incomplete or truncated stream",
        ));
    }
    let id = heap.allocate(HeapData::Bytes(Bytes::new(output)))?;
    Ok(Value::Ref(id))
}

/// Implements `zlib.compressobj(...)`.
fn zlib_compressobj(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    let pos_len = pos.len();
    if pos_len > 6 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("compressobj", 6, pos_len));
    }

    let mut level = -1_i64;
    let mut method = ZLIB_DEFLATED;
    let mut wbits = 15_i64;
    let mut mem_level = 8_i64;
    let mut strategy = 0_i64;
    let mut zdict: Option<Vec<u8>> = None;

    let mut from_pos = [false; 6];
    for (index, slot) in (0..6).zip([
        &mut level,
        &mut method,
        &mut wbits,
        &mut mem_level,
        &mut strategy,
        &mut 0_i64, // placeholder for zdict positional marker
    ]) {
        if let Some(value) = pos.next() {
            from_pos[index] = true;
            if index == 5 {
                zdict = Some(extract_bytes_like(&value, heap, interns)?);
                value.drop_with_heap(heap);
            } else {
                *slot = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
        }
    }

    for (key, value) in kwargs {
        let key_name = kwarg_name_from_value(&key, heap, interns)?;
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "level" => {
                if from_pos[0] {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compressobj", "level"));
                }
                level = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "method" => {
                if from_pos[1] {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compressobj", "method"));
                }
                method = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "wbits" => {
                if from_pos[2] {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compressobj", "wbits"));
                }
                wbits = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "memLevel" => {
                if from_pos[3] {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compressobj", "memLevel"));
                }
                mem_level = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "strategy" => {
                if from_pos[4] {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compressobj", "strategy"));
                }
                strategy = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "zdict" => {
                if from_pos[5] {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("compressobj", "zdict"));
                }
                zdict = Some(extract_bytes_like(&value, heap, interns)?);
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword("compressobj", key_name.as_str()));
            }
        }
    }

    if !(level == -1 || (0..=9).contains(&level))
        || method != ZLIB_DEFLATED
        || !(1..=9).contains(&mem_level)
        || !(0..=4).contains(&strategy)
    {
        return Err(zlib_invalid_initialization_option());
    }
    let _ = parse_compress_wbits(wbits, true)?;

    let obj = ZlibCompressObject {
        level,
        method,
        wbits,
        mem_level,
        strategy,
        zdict,
        actions: Vec::new(),
    };
    let id = heap.allocate(HeapData::ZlibCompress(obj))?;
    Ok(Value::Ref(id))
}

/// Implements `zlib.decompressobj(wbits=15, zdict=b'')`.
fn zlib_decompressobj(heap: &mut Heap<impl ResourceTracker>, interns: &Interns, args: ArgValues) -> RunResult<Value> {
    let (mut pos, kwargs) = args.into_parts();
    let pos_len = pos.len();
    if pos_len > 2 {
        pos.drop_with_heap(heap);
        kwargs.drop_with_heap(heap);
        return Err(ExcType::type_error_at_most("decompressobj", 2, pos_len));
    }

    let mut wbits = 15_i64;
    let mut zdict = Vec::new();
    let mut wbits_from_pos = false;
    let mut zdict_from_pos = false;

    if let Some(value) = pos.next() {
        wbits = value.as_int(heap)?;
        wbits_from_pos = true;
        value.drop_with_heap(heap);
    }
    if let Some(value) = pos.next() {
        zdict = extract_zdict_for_decompressobj(&value, heap, interns)?;
        zdict_from_pos = true;
        value.drop_with_heap(heap);
    }

    for (key, value) in kwargs {
        let key_name = kwarg_name_from_value(&key, heap, interns)?;
        key.drop_with_heap(heap);
        match key_name.as_str() {
            "wbits" => {
                if wbits_from_pos {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("decompressobj", "wbits"));
                }
                wbits = value.as_int(heap)?;
                value.drop_with_heap(heap);
            }
            "zdict" => {
                if zdict_from_pos {
                    value.drop_with_heap(heap);
                    return Err(ExcType::type_error_duplicate_arg("decompressobj", "zdict"));
                }
                zdict = extract_zdict_for_decompressobj(&value, heap, interns)?;
                value.drop_with_heap(heap);
            }
            _ => {
                value.drop_with_heap(heap);
                return Err(ExcType::type_error_unexpected_keyword(
                    "decompressobj",
                    key_name.as_str(),
                ));
            }
        }
    }

    let _ = parse_decompress_wbits(wbits, true)?;
    let obj = ZlibDecompressObject {
        wbits,
        zdict,
        actions: Vec::new(),
    };
    let id = heap.allocate(HeapData::ZlibDecompress(obj))?;
    Ok(Value::Ref(id))
}

/// Parses `wbits` for compression.
fn parse_compress_wbits(wbits: i64, strict_error: bool) -> RunResult<(CompressMode, u8)> {
    let result = if (-15..=-9).contains(&wbits) {
        (CompressMode::Raw, (-wbits).max(9) as u8)
    } else if (8..=15).contains(&wbits) {
        (CompressMode::Zlib, wbits.max(9) as u8)
    } else if (25..=31).contains(&wbits) {
        (CompressMode::Gzip, (wbits - 16).max(9) as u8)
    } else {
        return if strict_error {
            Err(zlib_invalid_initialization_option())
        } else {
            Err(zlib_error("Bad compression level"))
        };
    };
    Ok(result)
}

/// Parses `wbits` for decompression.
fn parse_decompress_wbits(wbits: i64, strict_error: bool) -> RunResult<(DecompressMode, u8)> {
    let result = if (-15..=-8).contains(&wbits) {
        (DecompressMode::Raw, (-wbits).max(9) as u8)
    } else if wbits == 0 {
        (DecompressMode::Zlib, 15)
    } else if (8..=15).contains(&wbits) {
        (DecompressMode::Zlib, wbits.max(9) as u8)
    } else if wbits == 16 {
        (DecompressMode::Gzip, 15)
    } else if (24..=31).contains(&wbits) {
        (DecompressMode::Gzip, (wbits - 16).max(9) as u8)
    } else if wbits == 32 {
        (DecompressMode::Auto, 15)
    } else if (40..=47).contains(&wbits) {
        (DecompressMode::Auto, (wbits - 32).max(9) as u8)
    } else {
        return if strict_error {
            Err(zlib_invalid_initialization_option())
        } else {
            Err(zlib_error(
                "Error -2 while preparing to decompress data: inconsistent stream state",
            ))
        };
    };
    Ok(result)
}

/// Builds a fresh compressor with the requested mode.
fn build_compressor(level: i64, mode: CompressMode, window_bits: u8) -> RunResult<Compress> {
    let compression = if level == -1 {
        Compression::default()
    } else {
        Compression::new(u32::try_from(level).unwrap_or(0))
    };
    let engine = match mode {
        CompressMode::Zlib => {
            let _ = window_bits;
            Compress::new(compression, true)
        }
        CompressMode::Raw => {
            let _ = window_bits;
            Compress::new(compression, false)
        }
        // The Rust `flate2` backend in this environment does not expose a gzip
        // in-memory streaming compressor. Keep stream behavior available by
        // falling back to zlib-wrapped output.
        CompressMode::Gzip => {
            let _ = window_bits;
            Compress::new(compression, true)
        }
    };
    Ok(engine)
}

/// Builds a fresh decompressor with the requested mode.
fn build_decompressor(mode: DecompressMode, window_bits: u8) -> Decompress {
    match mode {
        DecompressMode::Zlib => {
            let _ = window_bits;
            Decompress::new(true)
        }
        DecompressMode::Raw => {
            let _ = window_bits;
            Decompress::new(false)
        }
        DecompressMode::Gzip => {
            let _ = window_bits;
            Decompress::new(true)
        }
        DecompressMode::Auto => {
            let _ = window_bits;
            Decompress::new(true)
        }
    }
}

/// Returns true when bytes start with the gzip magic header.
fn is_gzip_header(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
}

/// One-shot gzip decompression helper used by `zlib.decompress`.
fn decompress_gzip_one_shot(data: &[u8]) -> RunResult<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut output = Vec::new();
    match decoder.read_to_end(&mut output) {
        Ok(_) => Ok(output),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                Err(zlib_error(
                    "Error -5 while decompressing data: incomplete or truncated stream",
                ))
            } else {
                Err(zlib_error("Error -3 while decompressing data: invalid input data"))
            }
        }
    }
}

/// Compresses `input` with `engine`, returning output bytes produced in this call.
fn compress_with_engine(
    engine: &mut Compress,
    input: &[u8],
    flush: FlushCompress,
    context: &str,
) -> RunResult<Vec<u8>> {
    let mut output = Vec::new();
    let mut input_offset = 0usize;

    loop {
        let mut out_buf = [0_u8; STREAM_CHUNK_SIZE];
        let before_in = engine.total_in();
        let before_out = engine.total_out();
        let status = engine
            .compress(&input[input_offset..], &mut out_buf, flush)
            .map_err(|_| zlib_error(format!("Error -2 while {context}: inconsistent stream state")))?;
        let consumed = usize::try_from(engine.total_in().saturating_sub(before_in)).unwrap_or(0);
        let produced = usize::try_from(engine.total_out().saturating_sub(before_out)).unwrap_or(0);

        input_offset = input_offset.saturating_add(consumed);
        output.extend_from_slice(&out_buf[..produced]);

        if matches!(status, Status::StreamEnd) {
            break;
        }
        let no_progress = consumed == 0 && produced == 0;
        if no_progress {
            break;
        }
        if input_offset >= input.len() && !matches!(flush, FlushCompress::Finish) {
            break;
        }
    }
    Ok(output)
}

/// Maps integer flush constants to `flate2` flush modes.
fn flush_compress_from_i64(mode: i64) -> Option<FlushCompress> {
    match mode {
        0 => Some(FlushCompress::None),
        1 => Some(FlushCompress::Partial),
        2 => Some(FlushCompress::Sync),
        3 => Some(FlushCompress::Full),
        4 => Some(FlushCompress::Finish),
        // `flate2` does not expose dedicated Block/Trees variants.
        // CPython accepts both modes; map them to `Sync` flush.
        5 => Some(FlushCompress::Sync),
        6 => Some(FlushCompress::Sync),
        _ => None,
    }
}

/// Extracts bytes from values accepted by zlib APIs.
fn extract_bytes_like(value: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::InternString(_) => Err(ExcType::type_error("a bytes-like object is required, not 'str'")),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Str(_) => Err(ExcType::type_error("a bytes-like object is required, not 'str'")),
            _ => Err(ExcType::type_error(format!(
                "a bytes-like object is required, not '{}'",
                value.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "a bytes-like object is required, not '{}'",
            value.py_type(heap)
        ))),
    }
}

/// Extracts `zdict` for `decompressobj`, matching CPython's error wording.
fn extract_zdict_for_decompressobj(
    value: &Value,
    heap: &Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Vec<u8>> {
    match value {
        Value::InternBytes(id) => Ok(interns.get_bytes(*id).to_vec()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Bytes(bytes) => Ok(bytes.as_slice().to_vec()),
            HeapData::Bytearray(bytes) => Ok(bytes.as_slice().to_vec()),
            _ => Err(ExcType::type_error("zdict argument must support the buffer protocol")),
        },
        _ => Err(ExcType::type_error("zdict argument must support the buffer protocol")),
    }
}

/// Converts a Python integer-like value into a `u32` mask (`mod 2**32`).
fn value_to_u32_mask(value: &Value, heap: &Heap<impl ResourceTracker>) -> RunResult<u32> {
    match value {
        Value::Int(i) => Ok(*i as u32),
        Value::Bool(b) => Ok(u32::from(*b)),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::LongInt(li) => {
                let modulus = BigInt::from(1_u64) << 32;
                let mut reduced: BigInt = li.inner() % &modulus;
                if reduced.is_negative() {
                    reduced += &modulus;
                }
                Ok(reduced.to_u32().unwrap_or(0))
            }
            _ => Ok(value.as_int(heap)? as u32),
        },
        _ => Ok(value.as_int(heap)? as u32),
    }
}

/// Computes CRC32 from an initial value and data bytes.
fn crc32_accumulate(initial: u32, bytes: &[u8]) -> u32 {
    let mut crc = !initial;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & 0xedb8_8320;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Extracts a keyword name from keyword mapping values.
fn kwarg_name_from_value(key: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match key {
        Value::InternString(id) => Ok(interns.get_str(*id).to_owned()),
        Value::Ref(id) => match heap.get(*id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            _ => Err(ExcType::type_error("keywords must be strings")),
        },
        _ => Err(ExcType::type_error("keywords must be strings")),
    }
}

/// Builds a `ValueError` with CPython's `Invalid initialization option` message.
fn zlib_invalid_initialization_option() -> RunError {
    SimpleException::new_msg(ExcType::ValueError, "Invalid initialization option").into()
}

/// Builds a `ValueError` with CPython's `Inconsistent stream state` message.
fn zlib_inconsistent_stream_state_value_error() -> RunError {
    SimpleException::new_msg(ExcType::ValueError, "Inconsistent stream state").into()
}

/// Builds a `zlib.error`-compatible error value (currently mapped to ValueError).
fn zlib_error(message: impl Into<String>) -> RunError {
    SimpleException::new_msg(ExcType::ValueError, message.into()).into()
}

/// Registers one module-level function.
fn register(
    module: &mut Module,
    name: &str,
    function: ZlibFunctions,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> Result<(), ResourceError> {
    module.set_attr_text(
        name,
        Value::ModuleFunction(ModuleFunctions::Zlib(function)),
        heap,
        interns,
    )
}
