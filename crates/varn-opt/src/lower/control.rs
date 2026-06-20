//! Control-flow statement emitters: `try`/`catch`/`finally`, the iteration
//! loops (`for-of`, `for-in`, `do-while`), and `switch`. Split from `stmt` to
//! keep the per-construct loop/handler machinery in one cohesive place.

use varn_core::OpCode;
use varn_types::chunk::Chunk;
use varn_types::value::RuntimeSymbol;

use super::FnLower;
use crate::hir::*;

impl FnLower {
    /// Emit `PopTry` + re-lower each pending `finally` body from `depth`..end
    /// (innermost first) before an early exit (`return`/`break`/`continue`).
    /// Mirrors legacy: the `finally_stack` is cloned so the in-flight finally
    /// bodies can be lowered while `self` is mutated.
    pub(super) fn run_finallys_from(&mut self, depth: usize) {
        if depth >= self.finally_stack.len() {
            return;
        }
        let fins: Vec<Vec<HirStmt>> = self.finally_stack[depth..].to_vec();
        for fin in fins.iter().rev() {
            self.chunk.emit(OpCode::PopTry, self.line);
            for s in fin {
                self.lower_stmt(s);
            }
        }
    }

    /// `try`/`catch`/`finally`. Split by shape so the semantics are correct on
    /// every exit path (the old single-path version swallowed exceptions in a
    /// `finally`-only `try`, and skipped `finally` on a `return` inside `catch`):
    ///
    /// * `finally` only → run `finally` on normal completion AND rethrow after it
    ///   on a thrown exception.
    /// * `catch` only → run the handler, binding the thrown value.
    /// * `catch` + `finally` → the handler wraps `catch` in its own `Try` so a
    ///   throw inside `catch` still runs `finally` then rethrows; `finally` runs
    ///   on the normal completion of both the `try` and the `catch` bodies; and a
    ///   `return`/`break`/`continue` inside either runs it via `finally_stack`.
    pub(super) fn lower_try(
        &mut self,
        block: &[HirStmt],
        catch: &Option<HirCatch>,
        finally: &Option<Vec<HirStmt>>,
    ) {
        match (catch, finally) {
            (None, Some(fin)) => self.lower_try_finally(block, fin),
            (Some(cc), None) => self.lower_try_catch(block, cc),
            (Some(cc), Some(fin)) => self.lower_try_catch_finally(block, cc, fin),
            // No handler at all (parser shouldn't produce this) — just the body.
            (None, None) => {
                for s in block {
                    self.lower_stmt(s);
                }
            }
        }
    }

    /// Emit `Try` installing a handler that writes the thrown value into a fresh
    /// `err_reg`; returns `(err_reg, patch_pos)` for the handler offset.
    fn emit_try(&mut self) -> (u8, usize) {
        let err_reg = self.alloc();
        self.chunk.emit(OpCode::Try, self.line);
        self.chunk.write(Chunk::pack(err_reg, 0), self.line);
        let patch = self.chunk.code.len();
        self.chunk.write(0xFFFF, self.line);
        self.chunk.write(0xFFFF, self.line);
        (err_reg, patch)
    }

    fn emit_throw_reg(&mut self, reg: u8) {
        self.chunk
            .emit1(OpCode::Throw, Chunk::pack(reg, 0), self.line);
    }

    fn bind_catch_param(&mut self, cc: &HirCatch, err_reg: u8) {
        if let Some(local) = cc.param {
            self.chunk
                .emit_rr(OpCode::Move, self.local_reg(local), err_reg, self.line);
        }
    }

    /// `try { block } finally { fin }`.
    fn lower_try_finally(&mut self, block: &[HirStmt], fin: &[HirStmt]) {
        self.finally_stack.push(fin.to_vec());
        let (err_reg, patch) = self.emit_try();
        for s in block {
            self.lower_stmt(s);
        }
        self.chunk.emit(OpCode::PopTry, self.line);
        self.finally_stack.pop();
        // Normal completion: run finally, skip the handler.
        for s in fin {
            self.lower_stmt(s);
        }
        let end_j = self.chunk.emit_jump(OpCode::Jump, self.line);
        // Handler (block threw): run finally, then rethrow.
        self.chunk.patch_jump(patch);
        for s in fin {
            self.lower_stmt(s);
        }
        self.emit_throw_reg(err_reg);
        self.chunk.patch_jump(end_j);
        self.free(); // err_reg
    }

    /// `try { block } catch (e) { body }`.
    fn lower_try_catch(&mut self, block: &[HirStmt], cc: &HirCatch) {
        let (err_reg, patch) = self.emit_try();
        for s in block {
            self.lower_stmt(s);
        }
        self.chunk.emit(OpCode::PopTry, self.line);
        let end_j = self.chunk.emit_jump(OpCode::Jump, self.line);
        // Handler (block threw): bind the error, run the catch body.
        self.chunk.patch_jump(patch);
        self.bind_catch_param(cc, err_reg);
        for s in &cc.body {
            self.lower_stmt(s);
        }
        self.chunk.patch_jump(end_j);
        self.free(); // err_reg
    }

    /// `try { block } catch (e) { body } finally { fin }`.
    fn lower_try_catch_finally(&mut self, block: &[HirStmt], cc: &HirCatch, fin: &[HirStmt]) {
        self.finally_stack.push(fin.to_vec());
        let (err_reg, patch) = self.emit_try();
        for s in block {
            self.lower_stmt(s);
        }
        self.chunk.emit(OpCode::PopTry, self.line);
        self.finally_stack.pop();
        // Block completed normally: run finally, go to end.
        for s in fin {
            self.lower_stmt(s);
        }
        let end_b = self.chunk.emit_jump(OpCode::Jump, self.line);

        // Handler (block threw): run the catch body wrapped in its own `Try` so a
        // throw inside `catch` still runs `finally` and rethrows.
        self.chunk.patch_jump(patch);
        self.finally_stack.push(fin.to_vec());
        let (err_reg2, patch2) = self.emit_try();
        self.bind_catch_param(cc, err_reg);
        for s in &cc.body {
            self.lower_stmt(s);
        }
        self.chunk.emit(OpCode::PopTry, self.line);
        self.finally_stack.pop();
        // Catch completed normally: run finally, go to end.
        for s in fin {
            self.lower_stmt(s);
        }
        let end_c = self.chunk.emit_jump(OpCode::Jump, self.line);
        // Handler2 (catch threw): run finally, rethrow.
        self.chunk.patch_jump(patch2);
        for s in fin {
            self.lower_stmt(s);
        }
        self.emit_throw_reg(err_reg2);

        self.chunk.patch_jump(end_b);
        self.chunk.patch_jump(end_c);
        self.free(); // err_reg2
        self.free(); // err_reg
    }

    /// `for (var of iterable) body` via the iterator protocol. Mirrors legacy
    /// `stmt.rs`: get `iterable[Symbol.iterator]()`, then each iteration call
    /// `iterator.next()`, exit when `.done`, bind `.value` to `var`. `iterator`
    /// and the reused `iter_fn` slot persist below the loop body's temporaries.
    /// `continue` re-runs the header (`Backward`).
    pub(super) fn lower_for_of(&mut self, var: LocalId, iterable: &HirExpr, body: &[HirStmt], is_await: bool) {
        let mark = self.next_temp;
        let iterable_reg = self.lower_expr(iterable);
        let sym_kind = if is_await {
            RuntimeSymbol::AsyncIterator
        } else {
            RuntimeSymbol::Iterator
        };
        let sym_idx = self.chunk.add_symbol(sym_kind);
        let iter_fn = self.alloc();
        self.chunk
            .emit_rrc(OpCode::GetSymbol, iter_fn, iterable_reg, sym_idx, self.line);
        let iterator = self.alloc();
        self.chunk.emit(OpCode::Call, self.line);
        self.chunk.write(Chunk::pack(iterator, iter_fn), self.line);
        self.chunk.write(Chunk::pack(1, iterable_reg), self.line);

        let loop_start = self.chunk.code.len();
        let next_key = self.chunk.add_str("next"); // MemberKey::IterNext
        self.emit_property(OpCode::GetProperty, iter_fn, iterator, next_key);
        let result_obj = self.alloc();
        self.chunk.emit(OpCode::Call, self.line);
        self.chunk.write(Chunk::pack(result_obj, iter_fn), self.line);
        self.chunk.write(Chunk::pack(1, iterator), self.line);

        let result_for_await = if is_await {
            let awaited = self.alloc();
            self.chunk.emit_rr(OpCode::Await, awaited, result_obj, self.line);
            awaited
        } else {
            result_obj
        };

        let done_key = self.chunk.add_str("done"); // MemberKey::IterDone
        let done_reg = self.alloc();
        self.emit_property(OpCode::GetProperty, done_reg, result_for_await, done_key);
        let exit = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, done_reg, self.line);

        let value_key = self.chunk.add_str("value"); // MemberKey::IterValue
        let value_reg = self.alloc();
        self.emit_property(OpCode::GetProperty, value_reg, result_for_await, value_key);
        self.chunk
            .emit_rr(OpCode::Move, self.local_reg(var), value_reg, self.line);
        // Free the per-iteration temps (result/done/value); keep iterator + the
        // reused iter_fn slot so the body's temps stack above them.
        self.free_to(iterator as u32 + 1);

        self.loops.push(super::LoopCtx {
            cont: super::ContinueMode::Backward(loop_start),
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            finally_depth: self.finally_stack.len(),
        });
        for s in body {
            self.lower_stmt(s);
        }
        let ctx = self.loops.pop().unwrap();
        self.chunk.emit_loop(loop_start, self.line);
        self.chunk.patch_jump(exit);
        for bj in ctx.break_jumps {
            self.chunk.patch_jump(bj);
        }
        self.free_to(mark);
    }

    /// `for (var in object) body` via `ObjectKeys` + index loop. Mirrors legacy
    /// `stmt.rs`: iterate `keys[0..keys.length]`, binding each key to `var`.
    /// `continue` jumps forward to the index increment (`Forward`).
    pub(super) fn lower_for_in(&mut self, var: LocalId, object: &HirExpr, body: &[HirStmt]) {
        let mark = self.next_temp;
        let keys = self.lower_expr(object);
        self.chunk.emit_rr(OpCode::ObjectKeys, keys, keys, self.line);
        let idx = self.alloc();
        self.chunk.emit_load_int(idx, 0, self.line);
        let len_reg = self.alloc();
        let length_key = self.chunk.add_str("length"); // MemberKey::Length

        let loop_start = self.chunk.code.len();
        self.emit_property(OpCode::GetProperty, len_reg, keys, length_key);
        let cond = self.alloc();
        self.chunk.emit_rrr(OpCode::Lt, cond, idx, len_reg, self.line);
        let exit = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
        self.free(); // cond
        let elem = self.alloc();
        self.chunk.emit_rrr(OpCode::GetIndex, elem, keys, idx, self.line);
        self.chunk
            .emit_rr(OpCode::Move, self.local_reg(var), elem, self.line);
        self.free(); // elem; body temps stack above len_reg

        self.loops.push(super::LoopCtx {
            cont: super::ContinueMode::Forward,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            finally_depth: self.finally_stack.len(),
        });
        for s in body {
            self.lower_stmt(s);
        }
        let ctx = self.loops.pop().unwrap();
        // `continue` lands here, just before the index increment.
        for cj in ctx.continue_jumps {
            self.chunk.patch_jump(cj);
        }
        let one = self.alloc();
        self.chunk.emit_load_int(one, 1, self.line);
        self.chunk.emit_rrr(OpCode::Add, idx, idx, one, self.line);
        self.free(); // one
        self.chunk.emit_loop(loop_start, self.line);
        self.chunk.patch_jump(exit);
        for bj in ctx.break_jumps {
            self.chunk.patch_jump(bj);
        }
        self.free_to(mark);
    }

    /// C-style `for`: test at the top, body, then the `update` (where `continue`
    /// lands via `Forward`, so it is never skipped), then loop. Mirrors legacy
    /// `stmt.rs` `For`. `init` is already lowered before this node.
    pub(super) fn lower_for_classic(
        &mut self,
        test: &HirExpr,
        update: &[HirStmt],
        body: &[HirStmt],
    ) {
        let loop_start = self.chunk.code.len();
        let mark = self.next_temp;
        let cond = self.lower_expr(test);
        let exit = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
        self.free_to(mark);
        self.loops.push(super::LoopCtx {
            cont: super::ContinueMode::Forward,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            finally_depth: self.finally_stack.len(),
        });
        for s in body {
            self.lower_stmt(s);
        }
        let ctx = self.loops.pop().unwrap();
        // `continue` lands here, before the update (so it isn't skipped).
        for cj in ctx.continue_jumps {
            self.chunk.patch_jump(cj);
        }
        for u in update {
            self.lower_stmt(u);
        }
        self.chunk.emit_loop(loop_start, self.line);
        self.chunk.patch_jump(exit);
        for bj in ctx.break_jumps {
            self.chunk.patch_jump(bj);
        }
    }

    /// `do body while (test)` — the body runs once before the test. `continue`
    /// jumps forward to the test (`Forward`); mirrors legacy `stmt.rs`.
    pub(super) fn lower_do_while(&mut self, body: &[HirStmt], test: &HirExpr) {
        let loop_start = self.chunk.code.len();
        self.loops.push(super::LoopCtx {
            cont: super::ContinueMode::Forward,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            finally_depth: self.finally_stack.len(),
        });
        for s in body {
            self.lower_stmt(s);
        }
        let ctx = self.loops.pop().unwrap();
        // `continue` lands here, at the trailing test.
        for cj in ctx.continue_jumps {
            self.chunk.patch_jump(cj);
        }
        let mark = self.next_temp;
        let cond = self.lower_expr(test);
        let exit = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
        self.free_to(mark);
        self.chunk.emit_loop(loop_start, self.line);
        self.chunk.patch_jump(exit);
        for bj in ctx.break_jumps {
            self.chunk.patch_jump(bj);
        }
    }

    /// `switch (disc) { cases }`. Two phases so fall-through is correct: first
    /// emit every case test (`disc == val` → `JumpIfTrue` to that case's body),
    /// then a jump to the `default` body (or past the switch); then emit the
    /// bodies in source order, which fall through to each other. A matched empty
    /// case therefore falls into the *next* body, not the next test (the bug in a
    /// naive interleaved chain). The switch is a `break`-only scope; `continue`
    /// targets the enclosing loop. `disc` stays live below the case temps.
    pub(super) fn lower_switch(&mut self, disc: &HirExpr, cases: &[HirSwitchCase]) {
        let mark = self.next_temp;
        let disc_reg = self.lower_expr(disc);

        // Phase 1: tests. Each non-default case gets a forward `JumpIfTrue` to
        // its body, patched in phase 2; the default index is remembered.
        let mut test_jumps: Vec<Option<usize>> = Vec::with_capacity(cases.len());
        let mut default_idx: Option<usize> = None;
        for (i, case) in cases.iter().enumerate() {
            match &case.test {
                Some(test) => {
                    let tmark = self.next_temp;
                    let test_r = self.lower_expr(test);
                    let eq = self.alloc();
                    self.chunk.emit_rrr(OpCode::Eq, eq, disc_reg, test_r, self.line);
                    let j = self.chunk.emit_cond_jump(OpCode::JumpIfTrue, eq, self.line);
                    self.free_to(tmark); // free test_r + eq
                    test_jumps.push(Some(j));
                }
                None => {
                    default_idx = Some(i);
                    test_jumps.push(None);
                }
            }
        }
        // No test matched → jump to the default body (patched when reached) or
        // past the switch.
        let no_match = self.chunk.emit_jump(OpCode::Jump, self.line);

        // Phase 2: bodies (source order, falling through).
        self.loops.push(super::LoopCtx {
            cont: super::ContinueMode::Skip,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            finally_depth: self.finally_stack.len(),
        });
        let mut default_patched = false;
        for (i, case) in cases.iter().enumerate() {
            if let Some(j) = test_jumps[i] {
                self.chunk.patch_jump(j);
            }
            if default_idx == Some(i) {
                self.chunk.patch_jump(no_match);
                default_patched = true;
            }
            for s in &case.body {
                self.lower_stmt(s);
            }
        }
        if !default_patched {
            self.chunk.patch_jump(no_match); // no default → fall past the switch
        }
        let ctx = self.loops.pop().unwrap();
        for bj in ctx.break_jumps {
            self.chunk.patch_jump(bj);
        }
        self.free_to(mark);
    }
}
