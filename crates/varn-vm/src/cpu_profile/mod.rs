mod capture;
mod sampler;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

use crate::vm::Vm;
use capture::Shared;
use sampler::Sampler;

const SLOTS_PER_RUN: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProfileFrame {
    pub function: String,
    pub file: String,
}

pub struct CpuProfile {
    pub hz: u32,
    pub samples: u64,
    pub dropped: u64,
    pub truncated: u64,
    pub compiled_samples: u64,
    pub wall: Duration,
    pub frames: Vec<ProfileFrame>,
    pub stacks: BTreeMap<Vec<u32>, u64>,
    pub lines: BTreeMap<(u32, u32), u64>,
    index: BTreeMap<ProfileFrame, u32>,
}

impl CpuProfile {
    pub fn new(hz: u32) -> Self {
        Self {
            hz,
            samples: 0,
            dropped: 0,
            truncated: 0,
            compiled_samples: 0,
            wall: Duration::ZERO,
            frames: Vec::new(),
            stacks: BTreeMap::new(),
            lines: BTreeMap::new(),
            index: BTreeMap::new(),
        }
    }

    fn intern(&mut self, frame: ProfileFrame) -> u32 {
        if let Some(id) = self.index.get(&frame) {
            return *id;
        }
        let id = self.frames.len() as u32;
        self.frames.push(frame.clone());
        self.index.insert(frame, id);
        id
    }
}

pub struct CpuProfiler {
    shared: Box<Shared>,
    sampler: Sampler,
    started: Instant,
    hz: u32,
}

impl CpuProfiler {
    pub fn start(vm: &mut Vm, hz: u32) -> Result<Self, String> {
        if hz == 0 {
            return Err("the sampling rate must be at least 1 Hz".to_owned());
        }
        vm.ctx.frames.reserve(crate::frame::MAX_CALL_DEPTH);
        let shared = Box::new(Shared::new(SLOTS_PER_RUN, &raw const vm.ctx.frames));
        let period = Duration::from_secs_f64(1.0 / f64::from(hz));
        let sampler = Sampler::start(&shared, period)?;
        Ok(Self {
            shared,
            sampler,
            started: Instant::now(),
            hz,
        })
    }

    pub fn finish(self, into: &mut CpuProfile, _vm: &Vm) {
        let wall = self.started.elapsed();
        self.sampler.stop();
        into.wall += wall;
        into.dropped += self.shared.dropped() as u64;
        let mut resolved: FxHashMap<(usize, usize), Vec<(u32, u32)>> = FxHashMap::default();
        for index in 0..self.shared.recorded() {
            let slot = self.shared.slot(index);
            let mut entries: Vec<capture::Entry> = slot.frames[..slot.depth as usize].to_vec();
            let compiled = varn_jit::stack_roots::locate(slot.pc);
            if let Some(site) = &compiled {
                let owner = site.owner;
                let exact = capture::Entry {
                    proto: owner,
                    ip: site.ip as usize,
                };
                match entries.last_mut() {
                    Some(top) if top.proto == owner => *top = exact,
                    Some(_) | None => entries.push(exact),
                }
            }
            into.compiled_samples += u64::from(compiled.is_some());
            let mut stack: Vec<u32> = Vec::new();
            let mut leaf: Option<(u32, u32)> = None;
            for entry in &entries {
                let chain = resolved
                    .entry((entry.proto, entry.ip))
                    .or_insert_with(|| {
                        let proto = unsafe { &*(entry.proto as *const varn_types::FunctionProto) };
                        let mut infos = Vec::new();
                        crate::exec::exceptions::frame_chain(proto, entry.ip, &mut infos);
                        infos
                            .into_iter()
                            .rev()
                            .map(|info| {
                                let id = into.intern(ProfileFrame {
                                    function: info.fn_name,
                                    file: info.file,
                                });
                                (id, info.line)
                            })
                            .collect()
                    })
                    .clone();
                stack.extend(chain.iter().map(|(id, _)| *id));
                leaf = chain.last().copied();
            }
            let (Some(leaf), false) = (leaf, stack.is_empty()) else {
                continue;
            };
            *into.lines.entry(leaf).or_insert(0) += 1;
            into.samples += 1;
            into.truncated += u64::from(slot.truncated);
            *into.stacks.entry(stack).or_insert(0) += 1;
        }
        debug_assert_eq!(self.hz, into.hz);
    }
}
