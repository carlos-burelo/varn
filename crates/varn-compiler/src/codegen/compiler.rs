use super::ir::{ImmValue, IrBuilder, RegId};
use crate::chunk::{Chunk, FunctionProto, PoolEntry};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_core::{OpCode, TypeAnnotations};

pub struct IrAnalysisResult {
    pub vreg_count: u16,

    pub bytecode_estimate: usize,

    pub vregs_used: usize,

    pub instruction_count: usize,

    pub max_concurrent_live: usize,

    pub optimal_register_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UpvalueDesc {
    pub is_local: bool,
    pub index: u16,
}

pub struct LoopCtx {
    pub start: usize,
    pub break_jumps: Vec<usize>,
    pub continue_jumps: Vec<usize>,
    pub finally_depth: usize,
}

#[derive(Clone, Debug)]
pub enum VarLoc {
    Reg(u8),
    // Like Reg but marked as captured by a closure — CloseUpvalue will be emitted on scope exit.
    Captured(u8),
    Upvalue(u8),
}

pub struct RegAlloc {
    pub next: u8,
    pub max: u8,
    pub used: [u64; 4],
    pub alloc_stack: Vec<u8>,

    pub next_vreg: u16,
    pub max_vreg: u16,
    pub physical_map: std::collections::HashMap<u16, u8>,
    pub physical_free: Option<Vec<u8>>,
    pub max_physical: u8,
}

impl RegAlloc {
    pub fn new() -> Self {
        Self {
            next: 0,
            max: 0,
            used: [0; 4],
            alloc_stack: Vec::new(),
            next_vreg: 0,
            max_vreg: 0,
            physical_map: std::collections::HashMap::new(),
            physical_free: None,
            max_physical: 0,
        }
    }

    pub fn alloc(&mut self) -> u8 {
        let mut r = 0u8;
        'search: loop {
            let word = (r / 64) as usize;
            let bit = r % 64;
            if (self.used[word] & (1 << bit)) == 0 {
                break 'search;
            }
            if r == 255 {
                panic!("register overflow: function uses more than 255 registers. Split the function into smaller parts.");
            }
            r += 1;
        }

        let word = (r / 64) as usize;
        let bit = r % 64;
        self.used[word] |= 1 << bit;
        self.alloc_stack.push(r);

        self.next = self.next.max(r + 1);
        if self.next > self.max {
            self.max = self.next;
        }
        r
    }

    pub fn free(&mut self) {
        if let Some(r) = self.alloc_stack.pop() {
            let word = (r / 64) as usize;
            let bit = r % 64;
            self.used[word] &= !(1 << bit);

            let mut highest = 0u8;
            let mut found = false;
            for i in (0..=255).rev() {
                let w = (i / 64) as usize;
                let b = i % 64;
                if (self.used[w] & (1 << b)) != 0 {
                    highest = i;
                    found = true;
                    break;
                }
            }
            self.next = if found { highest + 1 } else { 0 };
        }
    }

    pub fn save(&self) -> u8 {
        self.alloc_stack.len() as u8
    }

    pub fn restore(&mut self, saved: u8) {
        while self.alloc_stack.len() > saved as usize {
            if let Some(r) = self.alloc_stack.pop() {
                let word = (r / 64) as usize;
                let bit = r % 64;
                self.used[word] &= !(1 << bit);
            }
        }
        let mut highest = 0u8;
        let mut found = false;
        for i in (0..=255).rev() {
            let w = (i / 64) as usize;
            let b = i % 64;
            if (self.used[w] & (1 << b)) != 0 {
                highest = i;
                found = true;
                break;
            }
        }
        self.next = if found { highest + 1 } else { 0 };
    }

    pub fn mark_used(&mut self, reg: u8) {
        let word = (reg / 64) as usize;
        let bit = reg % 64;
        self.used[word] |= 1 << bit;
        self.next = self.next.max(reg + 1);
        if self.next > self.max {
            self.max = self.next;
        }
    }

    pub fn alloc_vreg(&mut self) -> u16 {
        let r = self.next_vreg;
        self.next_vreg += 1;
        if self.next_vreg > self.max_vreg {
            self.max_vreg = self.next_vreg;
        }
        r
    }

    pub fn free_vreg(&mut self) {
        if self.next_vreg > 0 {
            self.next_vreg -= 1;
        }
    }

    pub fn save_vreg(&self) -> u16 {
        self.next_vreg
    }

    pub fn restore_vreg(&mut self, saved: u16) {
        self.next_vreg = saved;
    }

    pub fn allocate_physical(&mut self, vreg: u16) -> Result<u8, &'static str> {
        if let Some(&phys) = self.physical_map.get(&vreg) {
            return Ok(phys);
        }

        let free = self.physical_free.get_or_insert_with(|| (0u8..=255u8).collect());
        if let Some(phys) = free.pop() {
            self.physical_map.insert(vreg, phys);
            if phys > self.max_physical {
                self.max_physical = phys;
            }
            Ok(phys)
        } else {
            Err("register allocation failed: no free physical registers")
        }
    }

    pub fn get_physical(&self, vreg: u16) -> Option<u8> {
        self.physical_map.get(&vreg).copied()
    }
}

pub struct Compiler<'a> {
    pub chunk: Chunk,

    pub scopes: Vec<FxHashMap<Rc<str>, VarLoc>>,

    pub regs: RegAlloc,

    pub scope_start_regs: Vec<u8>,

    pub ir_builder: Option<IrBuilder>,

    pub phys_to_vreg: FxHashMap<u8, RegId>,

    pub is_global: bool,
    pub current_class: Option<Rc<str>>,
    pub current_superclass: Option<Rc<str>>,

    pub pending_field_inits: Vec<(Rc<str>, varn_core::ast::Expr)>,

    pub disposables: Vec<(u8, bool, usize)>,

    pub upvalues: Vec<UpvalueDesc>,
    pub loop_stack: Vec<LoopCtx>,
    pub finally_stack: Vec<varn_core::ast::Stmt>,

    pub protos: Rc<RefCell<Vec<FunctionProto>>>,

    pub is_async: bool,
    pub is_generator: bool,
    pub has_this: bool,
    pub arity: usize,
    pub has_rest: bool,

    pub source_file: Rc<str>,
    pub name: Rc<str>,

    pub annotations: &'a TypeAnnotations,
    pub extension_calls: &'a FxHashMap<u32, Rc<str>>,
    pub extension_members: &'a FxHashMap<u32, Rc<str>>,
    pub extension_set_members: &'a FxHashMap<u32, Rc<str>>,

    pub cache_count: u16,
    pub line: u32,

    // Non-null means there is a parent compiler (this is a nested function).
    // SAFETY: parent is always a stack-allocated Compiler that outlives this child,
    // because compile_function() creates child inside the parent's call frame and
    // joins before returning. No aliasing occurs: only resolve_upvalue accesses parent,
    // and it is always called from within the child's compilation (child is borrowed mut,
    // parent pointer is accessed only through the raw pointer, no second borrow exists).
    pub parent: *mut Compiler<'a>,
    pub parent_depth: usize,
    pub export_names: Vec<Rc<str>>,
}

impl<'a> Compiler<'a> {
    pub fn new_module(
        source_file: Rc<str>,
        annotations: &'a TypeAnnotations,
        extension_calls: &'a FxHashMap<u32, Rc<str>>,
        extension_members: &'a FxHashMap<u32, Rc<str>>,
        extension_set_members: &'a FxHashMap<u32, Rc<str>>,
        protos: Rc<RefCell<Vec<FunctionProto>>>,
        export_names: Vec<Rc<str>>,
    ) -> Self {
        let mut chunk = Chunk::new();
        chunk.source_file = source_file.clone();
        Self {
            chunk,
            scopes: vec![FxHashMap::default()],
            regs: RegAlloc::new(),
            scope_start_regs: vec![0],
            ir_builder: None,
            phys_to_vreg: FxHashMap::default(),
            is_global: true,
            current_class: None,
            current_superclass: None,
            pending_field_inits: Vec::new(),
            disposables: Vec::new(),
            upvalues: Vec::new(),
            loop_stack: Vec::new(),
            finally_stack: Vec::new(),
            protos,
            is_async: false,
            is_generator: false,
            has_this: false,
            arity: 0,
            has_rest: false,
            source_file,
            name: Rc::from("<module>"),
            annotations,
            extension_calls,
            extension_members,
            extension_set_members,
            cache_count: 0,
            line: 0,
            parent: std::ptr::null_mut(),
            parent_depth: 0,
            export_names,
        }
    }

    pub fn new_function(
        name: Rc<str>,
        source_file: Rc<str>,
        is_async: bool,
        is_generator: bool,
        has_this: bool,
        annotations: &'a TypeAnnotations,
        extension_calls: &'a FxHashMap<u32, Rc<str>>,
        extension_members: &'a FxHashMap<u32, Rc<str>>,
        extension_set_members: &'a FxHashMap<u32, Rc<str>>,
        protos: Rc<RefCell<Vec<FunctionProto>>>,
        parent: *mut Compiler<'a>,
        parent_depth: usize,
        current_class: Option<Rc<str>>,
        current_superclass: Option<Rc<str>>,
    ) -> Self {
        let mut chunk = Chunk::new();
        chunk.source_file = source_file.clone();
        Self {
            chunk,
            scopes: vec![FxHashMap::default()],
            regs: RegAlloc::new(),
            scope_start_regs: vec![0],
            ir_builder: None,
            phys_to_vreg: FxHashMap::default(),
            is_global: false,
            current_class,
            current_superclass,
            pending_field_inits: Vec::new(),
            disposables: Vec::new(),
            upvalues: Vec::new(),
            loop_stack: Vec::new(),
            finally_stack: Vec::new(),
            protos,
            is_async,
            is_generator,
            has_this,
            arity: 0,
            has_rest: false,
            source_file,
            name,
            annotations,
            extension_calls,
            extension_members,
            extension_set_members,
            cache_count: 0,
            line: 0,
            parent,
            parent_depth,
            export_names: Vec::new(),
        }
    }

    pub fn emit(&mut self, op: OpCode) {
        self.chunk.emit(op, self.line);
    }

    pub fn emit1(&mut self, op: OpCode, a: u16) {
        self.chunk.emit1(op, a, self.line);
    }

    pub fn emit2(&mut self, op: OpCode, a: u16, b: u16) {
        self.chunk.emit2(op, a, b, self.line);
    }

    pub fn emit3(&mut self, op: OpCode, a: u16, b: u16, c: u16) {
        self.chunk.emit3(op, a, b, c, self.line);
    }

    pub fn emit4(&mut self, op: OpCode, a: u16, b: u16, c: u16, d: u16) {
        self.chunk.emit4(op, a, b, c, d, self.line);
    }

    pub fn emit_rr(&mut self, op: OpCode, dest: u8, src: u8) {
        self.chunk.emit_rr(op, dest, src, self.line);
    }

    pub fn emit_rrr(&mut self, op: OpCode, dest: u8, src1: u8, src2: u8) {
        self.chunk.emit_rrr(op, dest, src1, src2, self.line);
    }

    pub fn emit_rrc(&mut self, op: OpCode, dest: u8, src: u8, const_idx: u16) {
        self.chunk.emit_rrc(op, dest, src, const_idx, self.line);
    }

    pub fn emit_property(&mut self, op: OpCode, dest: u8, src: u8, name_idx: u16) {
        let cs_idx = self.alloc_cache();
        debug_assert!(
            cs_idx <= 255,
            "IC cache slot overflow (>255) — function too large"
        );
        self.chunk
            .emit_rrc_ic(op, dest, src, name_idx, cs_idx as u8, self.line);
    }

    pub fn emit_rc(&mut self, op: OpCode, dest: u8, const_idx: u16) {
        self.chunk.emit_rc(op, dest, const_idx, self.line);
    }

    pub fn emit_load_int(&mut self, dest: u8, n: i64) {
        self.chunk.emit_load_int(dest, n, self.line);
    }

    pub fn emit_jump(&mut self, op: OpCode) -> usize {
        self.chunk.emit_jump(op, self.line)
    }

    pub fn emit_cond_jump(&mut self, op: OpCode, cond_reg: u8) -> usize {
        self.chunk.emit_cond_jump(op, cond_reg, self.line)
    }

    pub fn patch_jump(&mut self, pos: usize) {
        self.chunk.patch_jump(pos);
    }

    pub fn emit_loop(&mut self, loop_start: usize) {
        self.chunk.emit_loop(loop_start, self.line);
    }

    pub fn enable_ir_collection(&mut self) {
        self.ir_builder = Some(IrBuilder::new());
    }

    pub fn emit_ir_unary(&mut self, opcode: OpCode, imm: ImmValue) -> RegId {
        if let Some(builder) = &mut self.ir_builder {
            let dest = builder.alloc_vreg();
            builder.emit_unary(opcode, dest, imm);
            dest
        } else {
            RegId::NONE
        }
    }

    pub fn emit_ir_binary(&mut self, opcode: OpCode, src1: RegId, src2: RegId) -> RegId {
        if let Some(builder) = &mut self.ir_builder {
            let dest = builder.alloc_vreg();
            builder.emit_binary(opcode, dest, src1, src2);
            dest
        } else {
            RegId::NONE
        }
    }

    pub fn emit_ir_unary_src(&mut self, opcode: OpCode, src1: RegId) -> RegId {
        if let Some(builder) = &mut self.ir_builder {
            let dest = builder.alloc_vreg();
            builder.emit_unary_src(opcode, dest, src1);
            dest
        } else {
            RegId::NONE
        }
    }

    pub fn finalize_ir(&mut self) -> Option<super::ir::IrModule> {
        self.ir_builder.take().map(|builder| builder.finish())
    }

    pub fn map_phys_to_vreg(&mut self, phys: u8, vreg: RegId) {
        self.phys_to_vreg.insert(phys, vreg);
    }

    pub fn get_vreg_for_phys(&self, phys: u8) -> Option<RegId> {
        self.phys_to_vreg.get(&phys).copied()
    }

    pub fn add_str(&mut self, s: &str) -> u16 {
        self.chunk.add_str(s)
    }

    pub fn add_const(&mut self, entry: PoolEntry) -> u16 {
        self.chunk.add_constant(entry)
    }

    pub fn add_int(&mut self, v: i64) -> u16 {
        self.chunk.add_int(v)
    }

    pub fn alloc_cache(&mut self) -> u16 {
        let r = self.cache_count;
        self.cache_count += 1;
        r
    }

    pub fn alloc_reg(&mut self) -> u8 {
        self.regs.alloc()
    }

    pub fn free_reg(&mut self) {
        self.regs.free();
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(FxHashMap::default());
        self.scope_start_regs.push(self.regs.save());
    }

    pub fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.last() {
            let mut captured_regs: Vec<u8> = scope
                .values()
                .filter_map(|loc| {
                    if let VarLoc::Captured(r) = loc {
                        Some(*r)
                    } else {
                        None
                    }
                })
                .collect();
            if !captured_regs.is_empty() {
                captured_regs.sort_unstable();
                let lowest = captured_regs[0];
                self.chunk
                    .emit1(OpCode::CloseUpvalue, lowest as u16, self.line);
            }
        }
        self.scopes.pop();
        let saved = self.scope_start_regs.pop().unwrap_or(0);
        self.regs.restore(saved);
    }

    pub fn define_local(&mut self, name: Rc<str>, reg: u8) {
        self.regs.mark_used(reg);
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, VarLoc::Reg(reg));
        }
    }

    pub fn resolve_local(&self, name: &str) -> Option<VarLoc> {
        for scope in self.scopes.iter().rev() {
            if let Some(loc) = scope.get(name) {
                return Some(loc.clone());
            }
        }
        None
    }

    pub fn emit_load_var(&mut self, name: &str, dest: u8) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(loc) = scope.get(name) {
                match loc.clone() {
                    VarLoc::Reg(src) | VarLoc::Captured(src) => {
                        if src != dest {
                            self.emit_rr(OpCode::Move, dest, src);
                        }
                        return true;
                    }
                    VarLoc::Upvalue(uv) => {
                        self.chunk.emit1(
                            OpCode::LoadUpvalue,
                            Chunk::pack(dest, uv) as u16,
                            self.line,
                        );
                        return true;
                    }
                }
            }
        }

        if let Some(uv) = self.resolve_upvalue(name) {
            self.chunk
                .emit1(OpCode::LoadUpvalue, Chunk::pack(dest, uv) as u16, self.line);
            return true;
        }
        false
    }

    pub fn emit_store_var(&mut self, name: &str, src: u8) -> bool {
        let loc = self.scopes.iter().rev().find_map(|s| s.get(name).cloned());
        if let Some(loc) = loc {
            match loc {
                VarLoc::Reg(dest) | VarLoc::Captured(dest) => {
                    if dest != src {
                        self.emit_rr(OpCode::Move, dest, src);
                    }
                    return true;
                }
                VarLoc::Upvalue(uv) => {
                    self.chunk
                        .emit1(OpCode::StoreUpvalue, Chunk::pack(uv, src) as u16, self.line);
                    return true;
                }
            }
        }
        if let Some(uv) = self.resolve_upvalue(name) {
            self.chunk
                .emit1(OpCode::StoreUpvalue, Chunk::pack(uv, src) as u16, self.line);

            if let Some(scope) = self.scopes.last_mut() {
                scope.insert(Rc::from(name), VarLoc::Upvalue(uv));
            }
            return true;
        }
        false
    }

    pub fn analyze_ir(&mut self) -> Option<IrAnalysisResult> {
        let ir_module = self.finalize_ir()?;

        let bytecode_estimate = ir_module.estimate_bytecode_size();
        let instruction_count = ir_module.instrs.len();
        let vregs_used_count = ir_module.used_vregs().len();
        let vreg_count = vregs_used_count as u16;

        let mut analyzer = super::liveness::LivenessAnalyzer::new();
        let live_ranges = analyzer.analyze_module(&ir_module);
        let max_concurrent_live = analyzer.max_concurrent_live();

        let graph = super::liveness::InterferenceGraph::from_live_ranges(&live_ranges);
        let optimal_register_count = graph.chromatic_number_upper_bound();

        Some(IrAnalysisResult {
            vreg_count,
            bytecode_estimate,
            vregs_used: vregs_used_count,
            instruction_count,
            max_concurrent_live,
            optimal_register_count,
        })
    }

    pub fn finish_module(self) -> FunctionProto {
        let mut proto = FunctionProto {
            name: Some(self.name),
            arity: self.arity,
            export_names: self.export_names,
            register_count: self.regs.max as u16,
            has_rest: self.has_rest,
            is_async: self.is_async,
            is_generator: self.is_generator,
            has_this: self.has_this,
            upvalue_count: self.upvalues.len(),
            cache_count: self.cache_count as usize,
            chunk: self.chunk,
            jit_entry: std::cell::Cell::new(None),
            jit_code: std::cell::RefCell::new(None),
        };
        super::regalloc_post::optimize_function(&mut proto);
        proto
    }

    pub fn finish_function(self) -> (FunctionProto, Vec<UpvalueDesc>) {
        let mut proto = FunctionProto {
            name: Some(self.name),
            arity: self.arity,
            export_names: Vec::new(),
            register_count: self.regs.max as u16,
            has_rest: self.has_rest,
            is_async: self.is_async,
            is_generator: self.is_generator,
            has_this: self.has_this,
            upvalue_count: self.upvalues.len(),
            cache_count: self.cache_count as usize,
            chunk: self.chunk,
            jit_entry: std::cell::Cell::new(None),
            jit_code: std::cell::RefCell::new(None),
        };
        super::regalloc_post::optimize_function(&mut proto);
        (proto, self.upvalues)
    }

    pub fn compile_program(&mut self, program: &varn_core::ast::Program) {
        use super::stmt::compile_stmts;
        compile_stmts(self, &program.body);
        let line = self.line;
        let ret_reg = self.regs.alloc();
        self.chunk.emit_rr(OpCode::LoadNull, ret_reg, 0, line);
        self.chunk
            .emit1(OpCode::Return, Chunk::pack(0, ret_reg) as u16, line);
    }

    fn resolve_upvalue(&mut self, name: &str) -> Option<u8> {
        if self.parent.is_null() {
            return None;
        }
        // SAFETY: See safety comment on the `parent` field. Parent outlives child,
        // and no other borrow of parent exists at this call site.
        let parent = unsafe { &mut *self.parent };
        for scope in parent.scopes.iter_mut().rev() {
            if let Some(loc) = scope.get_mut(name) {
                match loc.clone() {
                    VarLoc::Reg(reg) | VarLoc::Captured(reg) => {
                        *loc = VarLoc::Captured(reg); // mark as captured so CloseUpvalue is emitted
                        return Some(self.add_upvalue(reg as u16, true));
                    }
                    VarLoc::Upvalue(uv) => {
                        return Some(self.add_upvalue(uv as u16, false));
                    }
                }
            }
        }
        if let Some(parent_uv) = parent.resolve_upvalue(name) {
            return Some(self.add_upvalue(parent_uv as u16, false));
        }
        None
    }

    fn add_upvalue(&mut self, index: u16, is_local: bool) -> u8 {
        for (i, uv) in self.upvalues.iter().enumerate() {
            if uv.index == index && uv.is_local == is_local {
                return i as u8;
            }
        }
        let i = self.upvalues.len() as u8;
        self.upvalues.push(UpvalueDesc { is_local, index });
        i
    }

    #[inline]
    pub fn alloc_slot(&mut self) -> u16 {
        self.alloc_reg() as u16
    }

    #[inline]
    pub fn free_slot(&mut self) {
        self.free_reg();
    }
}
