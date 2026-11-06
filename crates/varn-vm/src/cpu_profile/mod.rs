mod activity;
mod capture;
mod sampler;

pub(crate) use activity::Activity;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

use crate::vm::Vm;
use capture::Shared;
use sampler::Sampler;

const SLOTS_PER_RUN: usize = 16_384;
const BUSY_FRACTION_DENOMINATOR: u64 = 10;

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
    pub collecting_samples: u64,
    pub compiling_samples: u64,
    pub waiting_samples: u64,
    pub runs: u64,
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
            collecting_samples: 0,
            compiling_samples: 0,
            waiting_samples: 0,
            runs: 0,
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

    fn waiting_flags(shared: &Shared) -> Vec<bool> {
        let mut flags = vec![false];
        flags.extend((1..shared.recorded()).map(|index| {
            let (now, before) = (shared.slot(index), shared.slot(index - 1));
            let spent = now.cpu.saturating_sub(before.cpu);
            let elapsed = now.at_ns.saturating_sub(before.at_ns);
            spent < elapsed / BUSY_FRACTION_DENOMINATOR
        }));
        flags
    }

    pub fn finish(self, into: &mut CpuProfile, _vm: &Vm) {
        let wall = self.started.elapsed();
        let waiting = {
            self.sampler.stop();
            Self::waiting_flags(&self.shared)
        };
        into.wall += wall;
        into.runs += 1;
        into.dropped += self.shared.dropped() as u64;
        let mut resolved: FxHashMap<(usize, usize), Vec<(u32, u32)>> = FxHashMap::default();
        for (index, idle_flag) in waiting.iter().enumerate() {
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
            let collecting = slot.activity & activity::COLLECTING_BIT != 0;
            let compiling = slot.activity & activity::COMPILING_BIT != 0;
            let idle = *idle_flag && !collecting && !compiling;
            into.collecting_samples += u64::from(collecting);
            into.compiling_samples += u64::from(compiling && !collecting);
            into.waiting_samples += u64::from(idle);
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
            let (Some(mut leaf), false) = (leaf, stack.is_empty()) else {
                continue;
            };
            let pseudo = if collecting {
                Some("<gc>")
            } else if compiling {
                Some("<jit compile>")
            } else if idle {
                Some("<waiting>")
            } else {
                None
            };
            if let Some(name) = pseudo {
                let id = into.intern(ProfileFrame {
                    function: name.to_owned(),
                    file: String::new(),
                });
                stack.push(id);
                leaf = (id, 0);
            }
            *into.lines.entry(leaf).or_insert(0) += 1;
            into.samples += 1;
            into.truncated += u64::from(slot.truncated);
            *into.stacks.entry(stack).or_insert(0) += 1;
        }
        debug_assert_eq!(self.hz, into.hz);
    }
}
