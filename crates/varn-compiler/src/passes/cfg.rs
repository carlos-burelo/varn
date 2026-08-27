use crate::ssa::ir::{Block, BlockId, InstKind, SsaFunc, Terminator};

pub fn simplify_and_compact(func: &mut SsaFunc) -> bool {
    let mut changed = false;

    changed |= fold_redundant_branches(func);

    changed |= jump_forwarding(func);

    changed |= merge_blocks(func);

    let discarded = compact_cfg(func);
    changed |= discarded;

    changed
}

fn get_reachable(func: &SsaFunc) -> Vec<BlockId> {
    let mut visited = vec![false; func.blocks.len()];
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(func.entry);
    visited[func.entry.0 as usize] = true;

    let mut reachable = Vec::new();
    while let Some(curr) = queue.pop_front() {
        reachable.push(curr);
        let block = func.block(curr);
        for inst in &block.insts {
            if let InstKind::Try { handler } = &inst.kind {
                if !visited[handler.0 as usize] {
                    visited[handler.0 as usize] = true;
                    queue.push_back(*handler);
                }
            }
        }
        for succ in succs(&block.term) {
            if !visited[succ.0 as usize] {
                visited[succ.0 as usize] = true;
                queue.push_back(succ);
            }
        }
    }
    reachable
}

fn succs(t: &Terminator) -> Vec<BlockId> {
    match t {
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
        Terminator::Jump { target, .. } => vec![*target],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
    }
}

fn fold_redundant_branches(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    for b_idx in 0..func.blocks.len() {
        let b_id = BlockId(b_idx as u32);
        let term = func.block(b_id).term.clone();
        if let Terminator::Branch {
            cond: _,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } = term
        {
            if then_blk == else_blk && then_args == else_args {
                func.blocks[b_id.0 as usize].term = Terminator::Jump {
                    target: then_blk,
                    args: then_args,
                };

                let preds = &mut func.blocks[then_blk.0 as usize].preds;
                if let Some(pos) = preds.iter().position(|p| *p == b_id) {
                    preds.remove(pos);
                }
                changed = true;
            }
        }
    }
    changed
}

/// Colapsa cadenas `a -> b -> c` cuando `b` es un bloque vacío que solo
/// salta a `c`: redirige a `a` directo a `c` y quita `b` de en medio.
///
/// `b.preds` puede traer aristas que esta función no sabe reescribir: la
/// arista de excepción de un `Try { handler: b }` en algún bloque no
/// terminador vive en `preds` (así se construyó el IR) pero no en ningún
/// `Terminator::Jump`/`Branch` que apunte a `b`, así que el bucle de abajo
/// nunca la encuentra para redirigirla. Si esa arista existe, `b` sigue
/// siendo un destino real (el handler) y su propio `jump c` (que no se
/// toca) sigue siendo una arista real hacia `c`. Por eso solo se quita de
/// `b.preds` — y de `c.preds`, y solo cuando `b` se queda sin ningún pred
/// real — lo que el bucle efectivamente demostró haber redirigido; nunca se
/// asume que enumerar los preds fue lo mismo que redirigirlos todos.
fn jump_forwarding(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let n = func.blocks.len();
    for b_idx in 0..n {
        let b_id = BlockId(b_idx as u32);
        if func.block(b_id).params.is_empty() && func.block(b_id).insts.is_empty() {
            if let Terminator::Jump { target: c_id, args } = func.block(b_id).term.clone() {
                if c_id != b_id {
                    let b_preds = func.block(b_id).preds.clone();
                    if b_preds.is_empty() {
                        continue;
                    }

                    let mut updates = Vec::new();
                    for a_id in b_preds {
                        let a_term = &func.blocks[a_id.0 as usize].term;
                        match a_term {
                            Terminator::Jump { target, .. } if *target == b_id => {
                                updates.push((
                                    a_id,
                                    Terminator::Jump {
                                        target: c_id,
                                        args: args.clone(),
                                    },
                                ));
                            }
                            Terminator::Branch {
                                cond,
                                then_blk,
                                then_args,
                                else_blk,
                                else_args,
                            } => {
                                if *then_blk == b_id || *else_blk == b_id {
                                    let new_then = if *then_blk == b_id { c_id } else { *then_blk };
                                    let new_then_args = if *then_blk == b_id {
                                        args.clone()
                                    } else {
                                        then_args.clone()
                                    };
                                    let new_else = if *else_blk == b_id { c_id } else { *else_blk };
                                    let new_else_args = if *else_blk == b_id {
                                        args.clone()
                                    } else {
                                        else_args.clone()
                                    };
                                    updates.push((
                                        a_id,
                                        Terminator::Branch {
                                            cond: *cond,
                                            then_blk: new_then,
                                            then_args: new_then_args,
                                            else_blk: new_else,
                                            else_args: new_else_args,
                                        },
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }

                    for (a_id, new_term) in &updates {
                        func.blocks[a_id.0 as usize].term = new_term.clone();
                        func.blocks[c_id.0 as usize].preds.push(*a_id);
                        changed = true;
                    }

                    // Solo se quitan de b_id.preds los preds que el bucle de
                    // arriba probó que redirigió (los que tenían una arista de
                    // terminador hacia b_id). Cualquier otro se queda: sigue
                    // siendo un pred real que este pase no sabe reescribir.
                    func.blocks[b_id.0 as usize]
                        .preds
                        .retain(|p| !updates.iter().any(|(a_id, _)| a_id == p));

                    // b_id deja de ser un pred real de c_id únicamente cuando
                    // ya no le queda ninguna arista entrante propia: si le
                    // queda una (la de excepción, por ejemplo), su `jump c_id`
                    // original sigue siendo una arista real hacia c_id.
                    if func.blocks[b_id.0 as usize].preds.is_empty() {
                        let c_preds = &mut func.blocks[c_id.0 as usize].preds;
                        if let Some(pos) = c_preds.iter().position(|p| *p == b_id) {
                            c_preds.remove(pos);
                        }
                    }
                }
            }
        }
    }
    changed
}

fn merge_blocks(func: &mut SsaFunc) -> bool {
    let mut changed = false;
    let n = func.blocks.len();
    for a_idx in 0..n {
        let a_id = BlockId(a_idx as u32);
        let term = func.block(a_id).term.clone();
        if let Terminator::Jump { target: b_id, args } = term {
            if b_id != a_id {
                let b_preds = &func.block(b_id).preds;
                if b_preds == &[a_id] {
                    let b_params = func.block(b_id).params.clone();
                    for (&param, &arg) in b_params.iter().zip(args.iter()) {
                        func.replace_all_uses(param, arg);
                    }

                    let b_insts = std::mem::take(&mut func.blocks[b_id.0 as usize].insts);
                    let b_term = std::mem::replace(
                        &mut func.blocks[b_id.0 as usize].term,
                        Terminator::Unreachable,
                    );

                    let handlers: Vec<BlockId> = b_insts
                        .iter()
                        .filter_map(|i| match &i.kind {
                            InstKind::Try { handler } => Some(*handler),
                            _ => None,
                        })
                        .collect();
                    func.blocks[a_id.0 as usize].insts.extend(b_insts);
                    for h in handlers {
                        for p in &mut func.blocks[h.0 as usize].preds {
                            if *p == b_id {
                                *p = a_id;
                            }
                        }
                    }

                    let succs = succs(&b_term);
                    for s in succs {
                        let preds = &mut func.blocks[s.0 as usize].preds;
                        for p in preds {
                            if *p == b_id {
                                *p = a_id;
                            }
                        }
                    }

                    // b_id queda completamente absorbido en a_id: el único
                    // pred que tenía (a_id, comprobado arriba) ya no lo
                    // referencia — su term acaba de reemplazarse por el de
                    // b_id. No dejar aquí un pred obsoleto: no es la causa de
                    // ningún bug conocido hoy, pero es la clase de dato
                    // colgante que convierte una corrupción de otro pase en
                    // un pánico de verify() en vez de en un bloque muerto
                    // que compact_cfg simplemente descarta.
                    func.blocks[b_id.0 as usize].preds.clear();

                    func.blocks[a_id.0 as usize].term = b_term;
                    changed = true;
                }
            }
        }
    }
    changed
}

fn compact_cfg(func: &mut SsaFunc) -> bool {
    let reachable = get_reachable(func);
    if reachable.len() == func.blocks.len() {
        return false;
    }

    let mut old_to_new = vec![None; func.blocks.len()];
    for (new_idx, &old_id) in reachable.iter().enumerate() {
        old_to_new[old_id.0 as usize] = Some(BlockId(new_idx as u32));
    }

    let mut new_blocks = Vec::with_capacity(reachable.len());
    for &old_id in &reachable {
        let mut block = std::mem::replace(
            &mut func.blocks[old_id.0 as usize],
            Block {
                params: Vec::new(),
                insts: Vec::new(),
                term: Terminator::Unreachable,
                term_line: 0,
                preds: Vec::new(),
            },
        );

        block.preds = block
            .preds
            .into_iter()
            .filter_map(|p| old_to_new[p.0 as usize])
            .collect();

        for inst in &mut block.insts {
            if let InstKind::Try { handler } = &mut inst.kind {
                *handler = old_to_new[handler.0 as usize].expect("reachable targets reachable");
            }
        }
        match &mut block.term {
            Terminator::Jump { target, .. } => {
                *target = old_to_new[target.0 as usize].expect("reachable targets reachable");
            }
            Terminator::Branch {
                then_blk, else_blk, ..
            } => {
                *then_blk = old_to_new[then_blk.0 as usize].expect("reachable targets reachable");
                *else_blk = old_to_new[else_blk.0 as usize].expect("reachable targets reachable");
            }
            _ => {}
        }

        new_blocks.push(block);
    }

    func.entry = BlockId(0);
    func.blocks = new_blocks;
    true
}
