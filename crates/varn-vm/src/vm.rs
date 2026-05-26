use crate::exec;
use crate::frame::{VmClosure, VmUpvalue, VmUpvalueInner};
use crate::globals::GlobalStore;
use crate::heap::Heap;
use crate::loader::ModuleLoader;
use crate::profile::{ProfileCounters, VmProfile};
use crate::value::VmValue;
use exec::calls;
use exec::ExecCtx;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use varn_core::{ModuleId, OpCode};
use varn_types::chunk::FunctionProto;
use varn_types::chunk::PoolEntry;
use varn_types::{Closure, Literal};

pub struct Vm {
    pub ctx: ExecCtx,
}

impl Vm {
    pub fn new(
        precompiled: Rc<rustc_hash::FxHashMap<ModuleId, Rc<varn_types::FunctionProto>>>,
    ) -> Self {
        let mut ctx = ExecCtx::new(GlobalStore::new());
        ctx.precompiled = precompiled;
        Self { ctx }
    }

    pub fn set_trace(&mut self, v: bool) {
        self.ctx.trace = v;
    }

    pub fn set_no_jit(&mut self, v: bool) {
        self.ctx.no_jit = v;
    }

    pub fn with_loader(mut self, loader: Rc<dyn ModuleLoader>) -> Self {
        self.ctx.loader = Some(loader);
        self
    }

    pub fn from_snapshot(
        globals: GlobalStore,
        heap: Heap,
        precompiled: Rc<rustc_hash::FxHashMap<ModuleId, Rc<varn_types::FunctionProto>>>,
    ) -> Self {
        let mut ctx = ExecCtx::new(globals);
        ctx.heap = heap;
        ctx.precompiled = precompiled;
        Self { ctx }
    }

    pub fn run(&mut self, closure: Rc<Closure>) -> Result<VmValue, crate::error::RuntimeError> {
        let constants = calls::resolve_constants(&closure.proto, &mut self.ctx.heap);
        let upvalues = closure
            .upvalues
            .iter()
            .map(|uv| {
                let inner = uv.inner.borrow();
                VmUpvalue {
                    inner: Rc::new(RefCell::new(VmUpvalueInner {
                        value: self.ctx.heap.intern(inner.value.clone()),
                        stack_slot: inner.location,
                    })),
                }
            })
            .collect();

        let nan_closure = Rc::new(VmClosure::with_upvalues(
            closure.proto.clone(),
            upvalues,
            Rc::new(constants),
        ));

        if self.ctx.frames.is_empty() {
            self.ctx.push_frame(nan_closure)?;
        }

        self.ctx.run()
    }

    pub fn snapshot(&self) -> (GlobalStore, Heap) {
        (self.ctx.globals.clone(), self.ctx.heap.clone())
    }

    pub fn enable_opcode_profiling(&mut self) {
        let mut v = Vec::with_capacity(512);
        for _ in 0..512 {
            v.push(AtomicU64::new(0));
        }
        self.ctx.opcode_counts = Some(Rc::new(v));
    }

    pub fn enable_profiling(&mut self) {
        self.ctx.profile_counters = Some(ProfileCounters::new());
    }

    pub fn take_profile(&mut self) -> Option<VmProfile> {
        self.ctx.profile_counters.take().map(|arc| {
            let profile = VmProfile::from_counters(&arc);
            VmProfile {
                heap_allocs: self.ctx.heap.alloc_count,
                gc_collections: self.ctx.heap.gc_collections,
                gc_freed: self.ctx.heap.gc_total_freed,
                heap_live: self.ctx.heap.live_count() as u64,
                heap_total: self.ctx.heap.objects_len() as u64,
                nursery_allocs: self.ctx.heap.nursery.alloc_count,
                minor_gc_count: self.ctx.heap.nursery.minor_gc_count,
                minor_gc_promoted: self.ctx.heap.nursery.minor_gc_promoted,
                ..profile
            }
        })
    }

    pub fn collect_gc(&mut self) -> usize {
        let roots: Vec<u32> = self
            .ctx
            .stack
            .iter()
            .chain(self.ctx.globals.values.iter())
            .filter(|v| v.is_heap())
            .map(|v| v.as_heap_idx())
            .collect();
        self.ctx.heap.collect(&roots).unwrap_or(0)
    }

    pub fn take_opcode_counts(&mut self) -> Vec<(OpCode, u64)> {
        let counts = match self.ctx.opcode_counts.take() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut result = Vec::new();
        for (i, c) in counts.iter().enumerate() {
            let val = c.load(Ordering::Relaxed);
            if val > 0 {
                if let Some(op) = OpCode::from_u16(i as u16) {
                    result.push((op, val));
                }
            }
        }
        result.sort_by_key(|(_, c)| Reverse(*c));
        result
    }

    pub fn resolve_globals(&mut self, proto: &mut FunctionProto) {
        resolve_globals_in_proto(proto, &mut self.ctx.globals);
    }
}

fn resolve_globals_in_proto(proto: &mut FunctionProto, globals: &mut GlobalStore) {
    let chunk = &mut proto.chunk;
    let mut ip = 0;
    while ip < chunk.code.len() {
        let raw = chunk.code[ip];
        let Some(op) = OpCode::from_u8(raw as u8) else {
            ip += 1;
            continue;
        };

        match op {
            OpCode::LoadGlobal => {
                if ip + 1 < chunk.code.len() {
                    let name_idx = chunk.code[ip + 1] as usize;
                    if let Some(PoolEntry::Literal(Literal::Str(name))) =
                        chunk.constants.get(name_idx)
                    {
                        let global_idx = if let Some(idx) = globals.resolve_index(name) {
                            idx
                        } else {
                            globals.define(name, VmValue::null())
                        };

                        let dest = chunk.code[ip] & 0xFF00;
                        chunk.code[ip] = dest | (OpCode::LoadGlobalIdx as u8 as u16);
                        chunk.code[ip + 1] = global_idx as u16;
                    }
                }
                ip += 2;
            }

            OpCode::StoreGlobal | OpCode::DefineGlobal => {
                if ip + 2 < chunk.code.len() {
                    let name_idx = chunk.code[ip + 2] as usize;
                    if let Some(PoolEntry::Literal(Literal::Str(name))) =
                        chunk.constants.get(name_idx)
                    {
                        let global_idx = if let Some(idx) = globals.resolve_index(name) {
                            idx
                        } else {
                            globals.define(name, VmValue::null())
                        };
                        let new_op = match op {
                            OpCode::StoreGlobal => OpCode::StoreGlobalIdx,
                            OpCode::DefineGlobal => OpCode::DefineGlobalIdx,
                            _ => unreachable!(),
                        };
                        chunk.code[ip] = new_op as u8 as u16;
                        chunk.code[ip + 2] = global_idx as u16;
                    }
                }
                ip += 3;
            }

            OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::Nop
            | OpCode::PopTry => {
                ip += 1;
            }

            OpCode::LoadInt
            | OpCode::LoadConst
            | OpCode::LoadGlobalIdx
            | OpCode::Move
            | OpCode::Negate
            | OpCode::Not
            | OpCode::ToString
            | OpCode::IsNull
            | OpCode::IsArray
            | OpCode::Typeof
            | OpCode::AssertNotNull
            | OpCode::WrapSpread
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::StrConcat
            | OpCode::StrSlice
            | OpCode::In
            | OpCode::Instanceof
            | OpCode::SetIndex
            | OpCode::GetIndex
            | OpCode::LoadModule
            | OpCode::StoreModuleSlot
            | OpCode::LoadUpvalue
            | OpCode::StoreUpvalue
            | OpCode::CloseUpvalue
            | OpCode::ArrayLength
            | OpCode::ArrayPush
            | OpCode::ArrayPop
            | OpCode::ArrayExtend
            | OpCode::ObjectKeys
            | OpCode::ObjectMerge
            | OpCode::GetEnumTag
            | OpCode::StrLength
            | OpCode::GetSuper
            | OpCode::BindMethod
            | OpCode::Yield
            | OpCode::Await
            | OpCode::Spawn
            | OpCode::Throw
            | OpCode::Return
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::Inherit => {
                ip += 2;
            }

            OpCode::Jump
            | OpCode::Loop
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Call
            | OpCode::CallSpread
            | OpCode::MakeClass
            | OpCode::Method
            | OpCode::DefineStatic
            | OpCode::DefineGetter
            | OpCode::DefineSetter
            | OpCode::DefineStaticGetter
            | OpCode::DefineStaticSetter
            | OpCode::StoreGlobalIdx
            | OpCode::DefineGlobalIdx
            | OpCode::BuildArray
            | OpCode::BuildObjectWithShape
            | OpCode::GetPropertyMaybe
            | OpCode::GetSymbol
            | OpCode::DeclareField
            | OpCode::MakeEnumVariant
            | OpCode::GetFixedField
            | OpCode::SetFixedField
            | OpCode::LoadModuleSlot => {
                ip += 3;
            }

            OpCode::GetProperty | OpCode::SetProperty => {
                ip += 3;
            }

            OpCode::Try => {
                ip += 4;
            }

            OpCode::InvokeVirtual | OpCode::CallMethod => {
                ip += 4;
            }

            OpCode::InvokeRuntimeStatic => {
                ip += 5;
            }

            OpCode::MakeClosure => {
                if ip + 1 < chunk.code.len() {
                    let w1 = chunk.code[ip + 1];
                    let uv_count = (w1 & 0xFF) as usize;
                    ip += 3 + uv_count;
                } else {
                    ip += 1;
                }
            }

            OpCode::BuildObject => {
                if ip + 1 < chunk.code.len() {
                    let w1 = chunk.code[ip + 1];
                    let count = (w1 & 0xFF) as usize;
                    ip += 2 + count * 2;
                } else {
                    ip += 1;
                }
            }

            OpCode::ObjectRest => {
                if ip + 2 < chunk.code.len() {
                    let w2 = chunk.code[ip + 2];
                    let skip_count = (w2 >> 8) as usize;
                    ip += 3 + skip_count;
                } else {
                    ip += 1;
                }
            }

            OpCode::AddImm | OpCode::SubImm => {
                ip += 2;
            }

            OpCode::BuildStr => {
                if ip + 1 < chunk.code.len() {
                    let count = (chunk.code[ip + 1] >> 8) as usize;
                    ip += 2 + count;
                } else {
                    ip += 1;
                }
            }
        }
    }

    for entry in &mut chunk.constants {
        if let PoolEntry::Function(ref mut nested) = entry {
            resolve_globals_in_proto(std::rc::Rc::make_mut(nested), globals);
        }
    }
}
