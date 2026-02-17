//! Builder for emitting bytecode during compilation.
//!
//! `CodeBuilder` provides methods for emitting opcodes and operands, handling
//! forward jumps with patching, and tracking source locations for tracebacks.

use std::collections::HashSet;

use super::{
    code::{Code, ConstPool, ExceptionEntry, LocationEntry},
    op::Opcode,
};
use crate::{intern::StringId, parse::CodeRange, value::Value};

/// Builder for emitting bytecode during compilation.
///
/// Handles encoding opcodes and operands into raw bytes, managing forward jumps
/// that need patching, and tracking source locations for traceback generation.
///
/// # Usage
///
/// ```ignore
/// let mut builder = CodeBuilder::new();
/// builder.set_location(some_range, None);
/// builder.emit(Opcode::LoadNone);
/// builder.emit_u8(Opcode::LoadLocal, 0);
/// let jump = builder.emit_jump(Opcode::JumpIfFalse);
/// // ... emit more code ...
/// builder.patch_jump(jump);
/// let code = builder.build(num_locals);
/// ```
#[derive(Debug, Default)]
pub struct CodeBuilder {
    /// The bytecode being built.
    bytecode: Vec<u8>,

    /// Constants collected during compilation.
    constants: Vec<Value>,

    /// Instruction start offsets in bytecode emission order.
    ///
    /// Used by final peephole optimization to rewrite opcode pairs safely
    /// without needing to decode instruction lengths from raw bytes.
    instruction_offsets: Vec<usize>,

    /// Source location entries for traceback generation.
    location_table: Vec<LocationEntry>,

    /// Exception handler entries.
    exception_table: Vec<ExceptionEntry>,

    /// Current source location (set before emitting instructions).
    current_location: Option<CodeRange>,

    /// Current focus location within the source range.
    current_focus: Option<CodeRange>,

    /// Current stack depth for tracking max stack usage.
    current_stack_depth: u16,

    /// Maximum stack depth seen during compilation.
    max_stack_depth: u16,

    /// Local variable names indexed by slot number.
    ///
    /// Populated during compilation to enable proper NameError messages
    /// when accessing undefined local variables.
    local_names: Vec<Option<StringId>>,

    /// Local variable slots that are assigned somewhere in this function.
    ///
    /// Used to determine whether to raise `UnboundLocalError` or `NameError`
    /// when loading an undefined local variable.
    assigned_locals: HashSet<u16>,
}

impl CodeBuilder {
    /// Creates a new empty CodeBuilder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the current source location for subsequent instructions.
    ///
    /// This location will be recorded in the location table when the next
    /// instruction is emitted. Call this before emitting instructions that
    /// correspond to source code.
    pub fn set_location(&mut self, range: CodeRange, focus: Option<CodeRange>) {
        self.current_location = Some(range);
        self.current_focus = focus;
    }

    /// Emits a no-operand instruction and updates stack depth tracking.
    pub fn emit(&mut self, op: Opcode) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        // Track stack effect for opcodes with known fixed effects
        if let Some(effect) = op.stack_effect() {
            self.adjust_stack(effect);
        }
    }

    /// Emits an instruction with a u8 operand and updates stack depth tracking.
    pub fn emit_u8(&mut self, op: Opcode, operand: u8) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        self.bytecode.push(operand);
        // Track stack effect - some need operand-based calculation
        self.track_stack_effect_u8(op, operand);
    }

    /// Emits an instruction with an i8 operand and updates stack depth tracking.
    pub fn emit_i8(&mut self, op: Opcode, operand: i8) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        // Reinterpret i8 as u8 for bytecode encoding
        self.bytecode.push(operand.to_ne_bytes()[0]);
        // Track stack effect for opcodes with known fixed effects
        if let Some(effect) = op.stack_effect() {
            self.adjust_stack(effect);
        }
    }

    /// Emits an instruction with two u8 operands and updates stack depth tracking.
    ///
    /// Used for UnpackEx: before_count (u8) + after_count (u8)
    pub fn emit_u8_u8(&mut self, op: Opcode, operand1: u8, operand2: u8) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        self.bytecode.push(operand1);
        self.bytecode.push(operand2);
        // UnpackEx: pops 1, pushes (before + 1 + after) = before + after + 1
        // Net effect: before + after
        if op == Opcode::UnpackEx {
            self.adjust_stack(i16::from(operand1) + i16::from(operand2));
        } else if let Some(effect) = op.stack_effect() {
            self.adjust_stack(effect);
        }
    }

    /// Emits an instruction with a u16 operand (little-endian) and updates stack depth tracking.
    pub fn emit_u16(&mut self, op: Opcode, operand: u16) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        self.bytecode.extend_from_slice(&operand.to_le_bytes());
        // Track stack effect - some need operand-based calculation
        self.track_stack_effect_u16(op, operand);
    }

    /// Emits an instruction with a u16 operand followed by a u8 operand.
    ///
    /// Used for MakeFunction: func_id (u16) + defaults_count (u8)
    /// Used for CallAttr: attr_name_id (u16) + arg_count (u8)
    pub fn emit_u16_u8(&mut self, op: Opcode, operand1: u16, operand2: u8) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        self.bytecode.extend_from_slice(&operand1.to_le_bytes());
        self.bytecode.push(operand2);
        // Track stack effects based on opcode
        match op {
            Opcode::MakeFunction => {
                // pops defaults_count defaults, pushes function: 1 - defaults_count
                self.adjust_stack(1 - i16::from(operand2));
            }
            Opcode::CallAttr => {
                // pops obj + args, pushes result: 1 - (1 + arg_count) = -arg_count
                self.adjust_stack(-i16::from(operand2));
            }
            _ => {
                if let Some(effect) = op.stack_effect() {
                    self.adjust_stack(effect);
                }
            }
        }
    }

    /// Emits an instruction with a u16 operand followed by two u8 operands.
    ///
    /// Used for MakeClosure: func_id (u16) + defaults_count (u8) + cell_count (u8)
    pub fn emit_u16_u8_u8(&mut self, op: Opcode, operand1: u16, operand2: u8, operand3: u8) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        self.bytecode.extend_from_slice(&operand1.to_le_bytes());
        self.bytecode.push(operand2);
        self.bytecode.push(operand3);
        // MakeClosure: pops defaults_count defaults, pushes closure
        // Cell values are captured from locals, not popped from stack
        // Stack effect: 1 - defaults_count
        if op == Opcode::MakeClosure {
            self.adjust_stack(1 - i16::from(operand2));
        } else if let Some(effect) = op.stack_effect() {
            self.adjust_stack(effect);
        }
    }

    /// Emits an instruction with two u16 operands followed by a u8 operand.
    ///
    /// Used for BuildClass: func_id (u16) + name_id (u16) + base_count (u8)
    pub fn emit_u16_u16_u8(&mut self, op: Opcode, operand1: u16, operand2: u16, operand3: u8) {
        self.start_instruction();
        self.bytecode.push(op as u8);
        self.bytecode.extend_from_slice(&operand1.to_le_bytes());
        self.bytecode.extend_from_slice(&operand2.to_le_bytes());
        self.bytecode.push(operand3);
        if op == Opcode::BuildClass {
            // Pops base_count bases from stack, pushes 1 ClassObject
            self.adjust_stack(1 - i16::from(operand3));
        } else if let Some(effect) = op.stack_effect() {
            self.adjust_stack(effect);
        }
    }

    /// Emits `CallBuiltinFunction` instruction.
    ///
    /// Operands: builtin_id (u8) + arg_count (u8)
    ///
    /// The builtin_id is the `#[repr(u8)]` discriminant of `BuiltinsFunctions`.
    /// This is an optimization that avoids constant pool lookup and stack manipulation.
    pub fn emit_call_builtin_function(&mut self, builtin_id: u8, arg_count: u8) {
        self.start_instruction();
        self.bytecode.push(Opcode::CallBuiltinFunction as u8);
        self.bytecode.push(builtin_id);
        self.bytecode.push(arg_count);
        // CallBuiltinFunction: pops args, pushes result. No callable on stack.
        // Stack effect: 1 - arg_count
        self.adjust_stack(1 - i16::from(arg_count));
    }

    /// Emits `CallBuiltinType` instruction.
    ///
    /// Operands: type_id (u8) + arg_count (u8)
    ///
    /// The type_id is the `#[repr(u8)]` discriminant of `BuiltinsTypes`.
    /// This is an optimization for type constructors like `list()`, `int()`, `str()`.
    pub fn emit_call_builtin_type(&mut self, type_id: u8, arg_count: u8) {
        self.start_instruction();
        self.bytecode.push(Opcode::CallBuiltinType as u8);
        self.bytecode.push(type_id);
        self.bytecode.push(arg_count);
        // CallBuiltinType: pops args, pushes result. No callable on stack.
        // Stack effect: 1 - arg_count
        self.adjust_stack(1 - i16::from(arg_count));
    }

    /// Emits CallFunctionKw with inline keyword names.
    ///
    /// Operands: pos_count (u8) + kw_count (u8) + kw_count * name_id (u16 each)
    ///
    /// The kwname_ids slice contains StringId indices for each keyword argument
    /// name, in order matching how the values were pushed to the stack.
    pub fn emit_call_function_kw(&mut self, pos_count: u8, kwname_ids: &[u16]) {
        self.start_instruction();
        self.bytecode.push(Opcode::CallFunctionKw as u8);
        self.bytecode.push(pos_count);
        self.bytecode
            .push(u8::try_from(kwname_ids.len()).expect("keyword count exceeds u8"));
        for &name_id in kwname_ids {
            self.bytecode.extend_from_slice(&name_id.to_le_bytes());
        }
        // CallFunctionKw: pops callable + pos_args + kw_args, pushes result
        // Stack effect: 1 - (1 + pos_count + kw_count) = -pos_count - kw_count
        let kw_count = i16::try_from(kwname_ids.len()).expect("keyword count exceeds i16");
        let total_args = i16::from(pos_count) + kw_count;
        self.adjust_stack(-total_args);
    }

    /// Emits CallAttrKw with inline keyword names.
    ///
    /// Operands: attr_name_id (u16) + pos_count (u8) + kw_count (u8) + kw_count * name_id (u16 each)
    ///
    /// The kwname_ids slice contains StringId indices for each keyword argument
    /// name, in order matching how the values were pushed to the stack.
    pub fn emit_call_attr_kw(&mut self, attr_name_id: u16, pos_count: u8, kwname_ids: &[u16]) {
        self.start_instruction();
        self.bytecode.push(Opcode::CallAttrKw as u8);
        self.bytecode.extend_from_slice(&attr_name_id.to_le_bytes());
        self.bytecode.push(pos_count);
        self.bytecode
            .push(u8::try_from(kwname_ids.len()).expect("keyword count exceeds u8"));
        for &name_id in kwname_ids {
            self.bytecode.extend_from_slice(&name_id.to_le_bytes());
        }
        // CallAttrKw: pops obj + pos_args + kw_args, pushes result
        // Stack effect: 1 - (1 + pos_count + kw_count) = -pos_count - kw_count
        let kw_count = i16::try_from(kwname_ids.len()).expect("keyword count exceeds i16");
        let total_args = i16::from(pos_count) + kw_count;
        self.adjust_stack(-total_args);
    }

    /// Emits a forward jump instruction, returning a label to patch later.
    ///
    /// The jump offset is initially set to 0 and must be patched with
    /// `patch_jump()` once the target location is known.
    #[must_use]
    pub fn emit_jump(&mut self, op: Opcode) -> JumpLabel {
        self.start_instruction();
        let label = JumpLabel(self.bytecode.len());
        self.bytecode.push(op as u8);
        // Placeholder for i16 offset (will be patched)
        self.bytecode.extend_from_slice(&0i16.to_le_bytes());
        // Track stack effect
        match op {
            // ForIter: when successful (not jumping), pushes next value (+1)
            // When exhausted (jumping), pops iterator (-1), but that's after loop
            Opcode::ForIter => self.adjust_stack(1),
            // JumpIfTrueOrPop/JumpIfFalseOrPop: pops when not jumping (fallthrough)
            Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop => self.adjust_stack(-1),
            _ => {
                if let Some(effect) = op.stack_effect() {
                    self.adjust_stack(effect);
                }
            }
        }
        label
    }

    /// Patches a forward jump to point to the current bytecode location.
    ///
    /// The offset is calculated relative to the position after the jump
    /// instruction's operand (i.e., where execution would continue if
    /// the jump is not taken).
    ///
    /// # Panics
    ///
    /// Panics if the jump offset exceeds i16 range (-32768..32767), which
    /// indicates the function is too large. This is a compile-time error
    /// rather than silent truncation.
    pub fn patch_jump(&mut self, label: JumpLabel) {
        let target = self.bytecode.len();
        // Offset is relative to position after the jump instruction (opcode + i16 = 3 bytes)
        let target_i64 = i64::try_from(target).expect("bytecode target exceeds i64");
        let label_i64 = i64::try_from(label.0).expect("bytecode label exceeds i64");
        let raw_offset = target_i64 - label_i64 - 3;
        let offset =
            i16::try_from(raw_offset).expect("jump offset exceeds i16 range (-32768..32767); function too large");
        let bytes = offset.to_le_bytes();
        self.bytecode[label.0 + 1] = bytes[0];
        self.bytecode[label.0 + 2] = bytes[1];
    }

    /// Emits a backward jump to a known target offset.
    ///
    /// Unlike forward jumps, backward jumps have a known target at emit time,
    /// so no patching is needed.
    pub fn emit_jump_to(&mut self, op: Opcode, target: usize) {
        self.start_instruction();
        let current = self.bytecode.len();
        // Offset is relative to position after this instruction (current + 3)
        let target_i64 = i64::try_from(target).expect("bytecode target exceeds i64");
        let current_i64 = i64::try_from(current).expect("bytecode offset exceeds i64");
        let raw_offset = target_i64 - (current_i64 + 3);
        let offset =
            i16::try_from(raw_offset).expect("jump offset exceeds i16 range (-32768..32767); function too large");
        self.bytecode.push(op as u8);
        self.bytecode.extend_from_slice(&offset.to_le_bytes());
        // Track stack effect (jump instructions pop condition)
        if let Some(effect) = op.stack_effect() {
            self.adjust_stack(effect);
        }
    }

    /// Returns the current bytecode offset.
    ///
    /// Use this to record loop start positions for backward jumps.
    #[must_use]
    pub fn current_offset(&self) -> usize {
        self.bytecode.len()
    }

    /// Emits `LoadLocal`, using specialized opcodes for slots 0-3.
    ///
    /// Slots 0-3 use zero-operand opcodes (`LoadLocal0`, etc.) for efficiency.
    /// Slots 4-255 use `LoadLocal` with a u8 operand.
    /// Slots 256+ use `LoadLocalW` with a u16 operand.
    /// Registers a local variable name for a given slot.
    ///
    /// This is called during compilation when we encounter a variable access.
    /// The name is used to generate proper NameError messages.
    pub fn register_local_name(&mut self, slot: u16, name: StringId) {
        let slot_idx = slot as usize;
        // Extend the vector if needed
        if slot_idx >= self.local_names.len() {
            self.local_names.resize(slot_idx + 1, None);
        }
        // Only set if not already set (first occurrence determines the name)
        if self.local_names[slot_idx].is_none() {
            self.local_names[slot_idx] = Some(name);
        }
    }

    /// Registers a local variable slot as "assigned" (vs undefined reference).
    ///
    /// Called during compilation for variables that are assigned somewhere in the function.
    /// Used at runtime to determine whether to raise `UnboundLocalError` (assigned local
    /// accessed before assignment) or `NameError` (name doesn't exist anywhere).
    pub fn register_assigned_local(&mut self, slot: u16) {
        self.assigned_locals.insert(slot);
    }

    /// Emits a `LoadLocal` instruction, using specialized variants for common slots.
    pub fn emit_load_local(&mut self, slot: u16) {
        match slot {
            0 => self.emit(Opcode::LoadLocal0),
            1 => self.emit(Opcode::LoadLocal1),
            2 => self.emit(Opcode::LoadLocal2),
            3 => self.emit(Opcode::LoadLocal3),
            _ => {
                if let Ok(s) = u8::try_from(slot) {
                    self.emit_u8(Opcode::LoadLocal, s);
                } else {
                    self.emit_u16(Opcode::LoadLocalW, slot);
                }
            }
        }
    }

    /// Emits `StoreLocal`, using specialized variants for common slots.
    pub fn emit_store_local(&mut self, slot: u16) {
        match slot {
            0 => self.emit(Opcode::StoreLocal0),
            1 => self.emit(Opcode::StoreLocal1),
            2 => self.emit(Opcode::StoreLocal2),
            3 => self.emit(Opcode::StoreLocal3),
            _ => {
                if let Ok(s) = u8::try_from(slot) {
                    self.emit_u8(Opcode::StoreLocal, s);
                } else {
                    self.emit_u16(Opcode::StoreLocalW, slot);
                }
            }
        }
    }

    /// Adds a constant to the pool, returning its index.
    ///
    /// # Panics
    ///
    /// Panics if the constant pool exceeds 65535 entries. This is a compile-time
    /// error indicating the function has too many constants.
    #[must_use]
    pub fn add_const(&mut self, value: Value) -> u16 {
        let idx = self.constants.len();
        let idx_u16 = u16::try_from(idx).expect("constant pool exceeds u16 range (65535); too many constants");
        self.constants.push(value);
        idx_u16
    }

    /// Adds an exception handler entry.
    ///
    /// Entries should be added in innermost-first order for nested try blocks.
    pub fn add_exception_entry(&mut self, entry: ExceptionEntry) {
        self.exception_table.push(entry);
    }

    /// Returns the current tracked stack depth.
    #[must_use]
    pub fn stack_depth(&self) -> u16 {
        self.current_stack_depth
    }

    /// Builds the final Code object.
    ///
    /// Consumes the builder and returns a Code object containing the
    /// compiled bytecode and all metadata.
    #[must_use]
    pub fn build(mut self, num_locals: u16) -> Code {
        self.peephole_optimize();

        // Convert local_names from Vec<Option<StringId>> to Vec<StringId>,
        // using StringId::default() for slots with no recorded name
        let local_names: Vec<StringId> = self.local_names.into_iter().map(Option::unwrap_or_default).collect();

        Code::new(
            self.bytecode,
            ConstPool::from_vec(self.constants),
            self.location_table,
            self.exception_table,
            num_locals,
            self.max_stack_depth,
            local_names,
            self.assigned_locals,
        )
    }

    /// Applies final bytecode peephole rewrites before emitting `Code`.
    ///
    /// This pass currently fuses hot patterns:
    /// - `LoadSmallInt <v>; StoreLocal{0..3}` -> `StoreLocalSmallInt <slot> <v>`
    /// - `LoadSmallInt <v>; StoreLocal <slot>` -> `StoreLocalSmallInt <slot> <v>; Nop`
    /// - `CompareEq; JumpIfFalse <off>` -> `CompareEqJumpIfFalse <off+1>; Nop`
    ///
    /// The rewrite preserves bytecode length so jump targets, exception ranges,
    /// and source location offsets remain stable.
    fn peephole_optimize(&mut self) {
        let jump_targets = self.collect_jump_targets();
        let mut idx = 0usize;
        while idx + 1 < self.instruction_offsets.len() {
            let ip = self.instruction_offsets[idx];
            let jump_ip = self.instruction_offsets[idx + 1];

            // Fuse `LoadSmallInt; StoreLocal*` when no control-flow edge targets
            // the second instruction boundary.
            if self.bytecode[ip] == Opcode::LoadSmallInt as u8 && jump_ip == ip + 2 && !jump_targets.contains(&jump_ip)
            {
                let small = self.bytecode[ip + 1];
                match self.bytecode[jump_ip] {
                    x if x == Opcode::StoreLocal0 as u8 => {
                        self.bytecode[ip] = Opcode::StoreLocalSmallInt as u8;
                        self.bytecode[ip + 1] = 0;
                        self.bytecode[ip + 2] = small;
                        idx += 2;
                        continue;
                    }
                    x if x == Opcode::StoreLocal1 as u8 => {
                        self.bytecode[ip] = Opcode::StoreLocalSmallInt as u8;
                        self.bytecode[ip + 1] = 1;
                        self.bytecode[ip + 2] = small;
                        idx += 2;
                        continue;
                    }
                    x if x == Opcode::StoreLocal2 as u8 => {
                        self.bytecode[ip] = Opcode::StoreLocalSmallInt as u8;
                        self.bytecode[ip + 1] = 2;
                        self.bytecode[ip + 2] = small;
                        idx += 2;
                        continue;
                    }
                    x if x == Opcode::StoreLocal3 as u8 => {
                        self.bytecode[ip] = Opcode::StoreLocalSmallInt as u8;
                        self.bytecode[ip + 1] = 3;
                        self.bytecode[ip + 2] = small;
                        idx += 2;
                        continue;
                    }
                    x if x == Opcode::StoreLocal as u8 => {
                        // Preserve bytecode length: 2+2 bytes -> 3+1 bytes.
                        let slot = self.bytecode[jump_ip + 1];
                        self.bytecode[ip] = Opcode::StoreLocalSmallInt as u8;
                        self.bytecode[ip + 1] = slot;
                        self.bytecode[ip + 2] = small;
                        self.bytecode[ip + 3] = Opcode::Nop as u8;
                        idx += 2;
                        continue;
                    }
                    _ => {}
                }
            }

            if self.bytecode[ip] == Opcode::CompareEq as u8
                && jump_ip == ip + 1
                && jump_ip + 2 < self.bytecode.len()
                && self.bytecode[jump_ip] == Opcode::JumpIfFalse as u8
                && !jump_targets.contains(&jump_ip)
            {
                let old_offset = i16::from_le_bytes([self.bytecode[jump_ip + 1], self.bytecode[jump_ip + 2]]);
                // The fused opcode starts 1 byte earlier than JumpIfFalse did.
                // Add 1 so the jump lands on the same absolute target.
                if let Some(new_offset) = old_offset.checked_add(1) {
                    let bytes = new_offset.to_le_bytes();
                    self.bytecode[ip] = Opcode::CompareEqJumpIfFalse as u8;
                    self.bytecode[ip + 1] = bytes[0];
                    self.bytecode[ip + 2] = bytes[1];
                    self.bytecode[ip + 3] = Opcode::Nop as u8;
                    idx += 2;
                    continue;
                }
            }

            idx += 1;
        }
    }

    /// Collects all absolute bytecode offsets that can be jump targets.
    ///
    /// Peephole rewrites must not remove an instruction at any of these offsets,
    /// otherwise incoming control-flow edges could land inside an operand.
    fn collect_jump_targets(&self) -> HashSet<usize> {
        let mut targets = HashSet::new();

        for &ip in &self.instruction_offsets {
            let opcode = Opcode::try_from(self.bytecode[ip]).expect("invalid opcode while collecting jump targets");
            if matches!(
                opcode,
                Opcode::Jump
                    | Opcode::JumpIfTrue
                    | Opcode::JumpIfFalse
                    | Opcode::JumpIfTrueOrPop
                    | Opcode::JumpIfFalseOrPop
                    | Opcode::ForIter
                    | Opcode::CompareEqJumpIfFalse
            ) {
                let lo = *self
                    .bytecode
                    .get(ip + 1)
                    .expect("truncated jump opcode while collecting targets");
                let hi = *self
                    .bytecode
                    .get(ip + 2)
                    .expect("truncated jump opcode while collecting targets");
                let offset = i16::from_le_bytes([lo, hi]);
                let base = i64::try_from(ip + 3).expect("jump base exceeds i64");
                let target = base + i64::from(offset);
                let target = usize::try_from(target).expect("jump target became negative or overflowed");
                targets.insert(target);
            }
        }

        for entry in &self.exception_table {
            let handler = usize::try_from(entry.handler()).expect("exception handler offset exceeds usize");
            targets.insert(handler);
        }

        targets
    }

    /// Records instruction start metadata before opcode emission.
    ///
    /// Every opcode emitter must call this exactly once so peephole rewrites can
    /// iterate instruction boundaries without decoding raw bytes.
    fn start_instruction(&mut self) {
        self.instruction_offsets.push(self.bytecode.len());
        self.record_location();
    }

    /// Records the current location in the location table if set.
    fn record_location(&mut self) {
        if let Some(range) = self.current_location {
            let offset = u32::try_from(self.bytecode.len()).expect("bytecode length exceeds u32");
            self.location_table
                .push(LocationEntry::new(offset, range, self.current_focus));
        }
    }

    /// Sets the current stack depth to an absolute value.
    ///
    /// Used when compiling code paths that branch and reconverge with different
    /// stack states (e.g., break/continue through finally blocks).
    /// Updates `max_stack_depth` if the new depth exceeds it.
    pub fn set_stack_depth(&mut self, depth: u16) {
        self.current_stack_depth = depth;
        self.max_stack_depth = self.max_stack_depth.max(depth);
    }

    /// Adjusts the stack depth by the given delta.
    ///
    /// Positive values indicate pushes, negative values indicate pops.
    /// Updates `max_stack_depth` if the new depth exceeds it.
    fn adjust_stack(&mut self, delta: i16) {
        let new_depth = i32::from(self.current_stack_depth) + i32::from(delta);
        // Stack depth shouldn't go negative (indicates compiler bug)
        debug_assert!(new_depth >= 0, "Stack depth went negative: {new_depth}");
        // Safe cast: new_depth is non-negative and stack won't exceed u16::MAX in practice
        self.current_stack_depth = u16::try_from(new_depth.max(0)).unwrap_or(u16::MAX);
        self.max_stack_depth = self.max_stack_depth.max(self.current_stack_depth);
    }

    /// Tracks stack effect for opcodes with u8 operand.
    ///
    /// For opcodes with variable effects (like `CallFunction`, `BuildList`),
    /// calculates the effect based on the operand.
    fn track_stack_effect_u8(&mut self, op: Opcode, operand: u8) {
        let effect: i16 = match op {
            // CallFunction pops (callable + args), pushes result: -(1 + arg_count) + 1 = -arg_count
            Opcode::CallFunction => -i16::from(operand),
            // UnpackSequence pops 1, pushes n: n - 1
            Opcode::UnpackSequence => i16::from(operand) - 1,
            // ListAppend/SetAdd pop value: -1 (depth operand doesn't affect stack count)
            Opcode::ListAppend | Opcode::SetAdd => -1,
            // DictSetItem pops key and value: -2
            Opcode::DictSetItem => -2,
            // Default: use fixed effect if available
            _ => op.stack_effect().unwrap_or(0),
        };
        self.adjust_stack(effect);
    }

    /// Tracks stack effect for opcodes with u16 operand.
    ///
    /// For opcodes with variable effects (like `BuildList`, `BuildTuple`),
    /// calculates the effect based on the operand.
    fn track_stack_effect_u16(&mut self, op: Opcode, operand: u16) {
        // Safe cast: operand won't exceed i16::MAX in practice (would be a huge list)
        let operand_i16 = operand.cast_signed();
        let effect: i16 = match op {
            // BuildList/BuildTuple/BuildSet: pop n, push 1: -(n - 1) = 1 - n
            Opcode::BuildList | Opcode::BuildTuple | Opcode::BuildSet => 1 - operand_i16,
            // BuildDict: pop 2n (key-value pairs), push 1: 1 - 2n
            Opcode::BuildDict => 1 - 2 * operand_i16,
            // BuildFString: pop n parts, push 1: 1 - n
            Opcode::BuildFString => 1 - operand_i16,
            // Default: use fixed effect if available
            _ => op.stack_effect().unwrap_or(0),
        };
        self.adjust_stack(effect);
    }

    /// Manually adjust stack depth for complex scenarios.
    ///
    /// Use this when the compiler knows the exact stack effect that can't
    /// be determined from the opcode alone (e.g., exception handlers pushing
    /// an exception value).
    pub fn adjust_stack_depth(&mut self, delta: i16) {
        self.adjust_stack(delta);
    }
}

/// Label for a forward jump that needs patching.
///
/// Stores the bytecode offset where the jump instruction was emitted.
/// Pass this to `patch_jump()` once the target location is known.
#[derive(Debug, Clone, Copy)]
pub struct JumpLabel(usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_basic() {
        let mut builder = CodeBuilder::new();
        builder.emit(Opcode::LoadNone);
        builder.emit(Opcode::Pop);

        let code = builder.build(0);
        assert_eq!(code.bytecode(), &[Opcode::LoadNone as u8, Opcode::Pop as u8]);
    }

    #[test]
    fn test_emit_u8_operand() {
        let mut builder = CodeBuilder::new();
        builder.emit_u8(Opcode::LoadLocal, 42);

        let code = builder.build(0);
        assert_eq!(code.bytecode(), &[Opcode::LoadLocal as u8, 42]);
    }

    #[test]
    fn test_emit_u16_operand() {
        let mut builder = CodeBuilder::new();
        builder.emit_u16(Opcode::LoadConst, 0x1234);

        let code = builder.build(0);
        assert_eq!(code.bytecode(), &[Opcode::LoadConst as u8, 0x34, 0x12]);
    }

    #[test]
    fn test_forward_jump() {
        let mut builder = CodeBuilder::new();
        let jump = builder.emit_jump(Opcode::Jump);
        builder.emit(Opcode::LoadNone); // 1 byte, skipped by jump
        builder.emit(Opcode::LoadNone); // 1 byte, skipped by jump
        builder.patch_jump(jump);
        builder.emit(Opcode::LoadNone); // Return value
        builder.emit(Opcode::ReturnValue);

        let code = builder.build(0);
        // Jump at offset 0, target at offset 5 (after 2x LoadNone)
        // Offset = 5 - 0 - 3 = 2
        assert_eq!(
            code.bytecode(),
            &[
                Opcode::Jump as u8,
                2,
                0, // i16 little-endian = 2
                Opcode::LoadNone as u8,
                Opcode::LoadNone as u8,
                Opcode::LoadNone as u8,
                Opcode::ReturnValue as u8,
            ]
        );
    }

    #[test]
    fn test_backward_jump() {
        let mut builder = CodeBuilder::new();
        let loop_start = builder.current_offset();
        builder.emit(Opcode::LoadNone); // offset 0, 1 byte
        builder.emit(Opcode::Pop); // offset 1, 1 byte
        builder.emit_jump_to(Opcode::Jump, loop_start); // offset 2, target 0

        let code = builder.build(0);
        // Jump at offset 2, target at offset 0
        // Offset = 0 - (2 + 3) = -5
        let expected_offset = (-5i16).to_le_bytes();
        assert_eq!(
            code.bytecode(),
            &[
                Opcode::LoadNone as u8,
                Opcode::Pop as u8,
                Opcode::Jump as u8,
                expected_offset[0],
                expected_offset[1],
            ]
        );
    }

    #[test]
    fn test_load_local_specialization() {
        let mut builder = CodeBuilder::new();
        builder.emit_load_local(0);
        builder.emit_load_local(1);
        builder.emit_load_local(2);
        builder.emit_load_local(3);
        builder.emit_load_local(4);
        builder.emit_load_local(256);

        let code = builder.build(0);
        assert_eq!(
            code.bytecode(),
            &[
                Opcode::LoadLocal0 as u8,
                Opcode::LoadLocal1 as u8,
                Opcode::LoadLocal2 as u8,
                Opcode::LoadLocal3 as u8,
                Opcode::LoadLocal as u8,
                4,
                Opcode::LoadLocalW as u8,
                0,
                1, // 256 in little-endian
            ]
        );
    }

    #[test]
    fn test_store_local_specialization() {
        let mut builder = CodeBuilder::new();
        // StoreLocal operations pop from stack, so push values first to keep stack depth non-negative
        for _ in 0..6 {
            builder.emit(Opcode::LoadNone);
        }
        builder.emit_store_local(0);
        builder.emit_store_local(1);
        builder.emit_store_local(2);
        builder.emit_store_local(3);
        builder.emit_store_local(4);
        builder.emit_store_local(256);

        let code = builder.build(0);
        // Find the StoreLocal opcodes in the bytecode (skip the 6 LoadNone opcodes)
        let bytecode = code.bytecode();
        let store_start = 6; // After 6 LoadNone opcodes
        assert_eq!(
            &bytecode[store_start..],
            &[
                Opcode::StoreLocal0 as u8,
                Opcode::StoreLocal1 as u8,
                Opcode::StoreLocal2 as u8,
                Opcode::StoreLocal3 as u8,
                Opcode::StoreLocal as u8,
                4,
                Opcode::StoreLocalW as u8,
                0,
                1, // 256 in little-endian
            ]
        );
    }

    #[test]
    fn test_add_const() {
        let mut builder = CodeBuilder::new();
        let idx1 = builder.add_const(Value::Int(42));
        let idx2 = builder.add_const(Value::None);

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
    }

    #[test]
    fn test_compare_eq_jump_if_false_peephole() {
        let mut builder = CodeBuilder::new();
        builder.emit(Opcode::LoadNone);
        builder.emit(Opcode::LoadNone);
        builder.emit(Opcode::CompareEq);
        let jump = builder.emit_jump(Opcode::JumpIfFalse);
        builder.emit(Opcode::LoadTrue);
        builder.patch_jump(jump);
        builder.emit(Opcode::ReturnValue);

        let code = builder.build(0);
        assert_eq!(
            code.bytecode(),
            &[
                Opcode::LoadNone as u8,
                Opcode::LoadNone as u8,
                Opcode::CompareEqJumpIfFalse as u8,
                2,
                0, // i16 little-endian = 2
                Opcode::Nop as u8,
                Opcode::LoadTrue as u8,
                Opcode::ReturnValue as u8,
            ]
        );
    }

    #[test]
    fn test_load_small_int_store_local_peephole() {
        let mut builder = CodeBuilder::new();
        builder.emit_i8(Opcode::LoadSmallInt, 7);
        builder.emit_store_local(0);
        builder.emit_i8(Opcode::LoadSmallInt, -3);
        builder.emit_store_local(4);

        let code = builder.build(5);
        assert_eq!(
            code.bytecode(),
            &[
                Opcode::StoreLocalSmallInt as u8,
                0,
                7u8,
                Opcode::StoreLocalSmallInt as u8,
                4,
                (-3i8).to_ne_bytes()[0],
                Opcode::Nop as u8,
            ]
        );
    }
}
