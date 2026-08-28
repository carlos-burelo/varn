//! Backward liveness over the bytecode CFG, used to size the GC flush set.
//!
//! [`super::alloc::flush_boxed`] spills registers to their `ctx.stack` home
//! slots so a collection can root and rewrite them, and `reload_boxed` brings
//! them back. Spilling a register no reader will ever look at again costs
//! three times over: it is CLIF instructions Cranelift has to compile (and
//! compilation dominates a cold run), a store and a load at run time, and it
//! is emitted around EVERY helper call. Without this analysis the flush set
//! was every non-float register in the frame, so a function with a dozen
//! never-written registers stored a dozen known zeros before each call.
//!
//! Over-approximating is safe here and under-approximating loses a live
//! object, so every uncertainty resolves towards "live":
//!
//! * Anything the decoder cannot describe — an undecodable opcode, an out of
//!   range register — collapses the whole analysis to "everything is live",
//!   which is exactly the behaviour this module replaces.
//! * An exception LEAVES CLIF CODE ENTIRELY and resumes in the interpreter at
//!   the handler's bytecode ip (`alloc::emit_try_push` hands the VM that ip),
//!   where registers are read back from the home slots. Any helper call can
//!   throw, so in a function containing a `Try` every catch ip is a successor
//!   of every instruction. That over-approximates the try region on purpose:
//!   matching `Try`/`PopTry` pairs would narrow it, and would also turn a
//!   nesting bug into silently freed live objects.
//! * Register 0 is the callee/receiver slot the frame protocol reads from
//!   `stack[base + 0]`, so it is live everywhere.
//!
//! Suspension needs no special case: `Yield`/`Await` flush the whole frame at
//! their own site rather than through the flush set, and a helper that parks
//! the frame (`LoadModule`) rewinds to its own ip, where the live-after set
//! is by definition what resumption reads.

use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;

/// Registers a safepoint at each instruction has to preserve: everything
/// readable after it, UNION everything it reads itself. The second half is
/// not redundant — a call-shaped instruction reads its argument window out of
/// the home slots rather than from passed values, so an argument whose last
/// use is this very call is live-in but not live-after and still has to be
/// flushed. Dropping it made `18-ranges.vn` read a stale `end`.
///
/// Indexed by bytecode ip; ips that are not instruction starts are never
/// queried.
pub(crate) struct Liveness {
    /// `words` bits per ip, or empty when the analysis bailed.
    after: Vec<u64>,
    words: usize,
    nregs: usize,
    /// Set when nothing could be proven and every register must be flushed.
    all_live: bool,
}

impl Liveness {
    /// The conservative answer: every register is live everywhere. Also what
    /// an alloc-free function gets — it has no `AllocCtx`, so it never flushes
    /// and never queries this, and running the fixpoint for it was pure cost
    /// (it measured ~10% of `bench_fib`).
    pub(super) fn everything() -> Self {
        Self {
            after: Vec::new(),
            words: 0,
            nregs: 0,
            all_live: true,
        }
    }

    /// Whether register `r` may still be read after the instruction at `ip`.
    pub(super) fn is_live_after(&self, ip: usize, r: usize) -> bool {
        if self.all_live || r == 0 || r >= self.nregs {
            return true;
        }
        let idx = ip * self.words + r / 64;
        match self.after.get(idx) {
            Some(w) => w & (1u64 << (r % 64)) != 0,
            // An ip the scan never reached is not an instruction start we
            // emit a safepoint at; answer conservatively anyway.
            None => true,
        }
    }
}

/// One instruction's shape, kept flat so the fixpoint does not re-decode.
/// Successors are inline: the most any instruction has is two (a conditional
/// jump), and a Vec per instruction showed up in the profile.
struct Instr {
    def: Option<u8>,
    uses: Vec<u8>,
    succs: [usize; 2],
    nsucc: u8,
}

/// `VARN_JIT_NO_LIVENESS=1` restores the pre-analysis flush set (every
/// non-float register, everywhere). This machine thermally throttles hard
/// enough that comparing two binaries is worthless, so the A/B has to live
/// inside one — same binary, alternated order.
fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("VARN_JIT_NO_LIVENESS").is_ok())
}

pub(super) fn analyze(code: &[u16], pool: &[PoolEntry], nregs: usize) -> Liveness {
    if nregs == 0 || code.is_empty() || disabled() {
        return Liveness::everything();
    }
    let words = nregs.div_ceil(64);
    let n = code.len();

    // ---- decode every instruction start and its successors ----
    let mut instrs: Vec<Option<Instr>> = (0..n).map(|_| None).collect();
    let mut starts: Vec<usize> = Vec::new();
    let mut catch_ips: Vec<usize> = Vec::new();
    let mut ip = 0usize;
    while ip < n {
        let Some(info) = decode(code, ip, pool) else {
            return Liveness::everything();
        };
        let Some(op) = OpCode::from_u8(code[ip] as u8) else {
            return Liveness::everything();
        };
        if info.len == 0 {
            return Liveness::everything();
        }
        let next = ip + info.len;
        let mut succs: Vec<usize> = Vec::new();
        match op {
            // Jump offsets are encoded exactly as the lowering's own block
            // scan reads them; keep the two in step.
            OpCode::Jump => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                succs.push(ip + 3 + off);
            }
            OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                succs.push(ip + 3 + off);
                succs.push(next);
            }
            OpCode::Loop => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                succs.push((ip + 3).saturating_sub(off));
            }
            // Ends the frame: nothing downstream can read a register.
            OpCode::Return => {}
            OpCode::Try => {
                if ip + 3 >= n {
                    return Liveness::everything();
                }
                let catch_off = ((code[ip + 2] as usize) << 16) | code[ip + 3] as usize;
                catch_ips.push(ip + 4 + catch_off);
                succs.push(next);
            }
            _ => succs.push(next),
        }
        succs.retain(|&s| s < n);
        let mut inline = [0usize; 2];
        for (slot, &s) in inline.iter_mut().zip(succs.iter()) {
            *slot = s;
        }
        instrs[ip] = Some(Instr {
            def: info.def,
            uses: info.uses,
            succs: inline,
            nsucc: succs.len() as u8,
        });
        starts.push(ip);
        ip = next;
    }

    // Every catch ip must be a real instruction start, or the region model
    // below is meaningless and the safe answer is "everything".
    for &c in &catch_ips {
        if c >= n || instrs[c].is_none() {
            return Liveness::everything();
        }
    }

    // ---- fixpoint: live_in[i] = (live_after[i] - def) | uses ----
    let mut after = vec![0u64; n * words];
    let mut live_in = vec![0u64; n * words];
    let set = |v: &mut [u64], ip: usize, r: usize| {
        v[ip * words + r / 64] |= 1u64 << (r % 64);
    };

    let mut changed = true;
    while changed {
        changed = false;
        for &i in starts.iter().rev() {
            let instr = instrs[i].as_ref().expect("start decoded above");

            // live_after = union of successors' live_in, plus every catch ip
            // when the frame has a handler: any call in here can throw, and
            // the handler reads its registers from the home slots.
            let mut acc = vec![0u64; words];
            for &s in &instr.succs[..instr.nsucc as usize] {
                for w in 0..words {
                    acc[w] |= live_in[s * words + w];
                }
            }
            for &c in &catch_ips {
                for w in 0..words {
                    acc[w] |= live_in[c * words + w];
                }
            }
            for (w, &acc_w) in acc.iter().enumerate().take(words) {
                let slot = i * words + w;
                if after[slot] != acc_w {
                    after[slot] = acc_w;
                    changed = true;
                }
            }

            let mut in_bits = acc;
            if let Some(d) = instr.def {
                let d = d as usize;
                if d < nregs {
                    in_bits[d / 64] &= !(1u64 << (d % 64));
                }
            }
            for &u in &instr.uses {
                let u = u as usize;
                if u >= nregs {
                    return Liveness::everything();
                }
                in_bits[u / 64] |= 1u64 << (u % 64);
            }
            for (w, &in_w) in in_bits.iter().enumerate().take(words) {
                let slot = i * words + w;
                if live_in[slot] != in_w {
                    live_in[slot] = in_w;
                    changed = true;
                }
            }
        }
    }

    // Fold live-in into the answer: see the type's docs for why a register
    // this instruction merely reads still has to reach its home slot.
    for &i in &starts {
        for w in 0..words {
            after[i * words + w] |= live_in[i * words + w];
        }
        // The receiver slot is read through `stack[base + 0]`, never through
        // a Variable, so the dataflow above cannot see those reads.
        set(&mut after, i, 0);
    }

    Liveness {
        after,
        words,
        nregs,
        all_live: false,
    }
}
