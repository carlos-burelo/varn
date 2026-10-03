use super::scan::ChildSlot;
use super::*;

impl Nursery {
    pub(crate) fn collect(
        &mut self,
        old_gen: &mut HeapInner,
        dyn_: &mut [VmValue],
        refs: &mut [u32],
        extra_root_packed: &[u32],
    ) {
        self.minor_gc_count += 1;
        let trace = crate::gc_trace::note_start(self.objects.len(), self.minor_gc_promoted);
        let mut worklist = std::mem::take(&mut self.worklist);
        worklist.clear();
        // Reused across every scanned object: one promoted object per scan
        // previously meant one fresh Vec allocation each.
        let mut fixups: Vec<(ChildSlot, u32)> = Vec::with_capacity(8);

        self.phase = "stack/ctx roots";
        for slot in dyn_.iter_mut() {
            self.update_value(slot, old_gen, &mut worklist);
        }

        self.phase = "ref roots";
        for slot in refs.iter_mut() {
            self.update_ref(slot, old_gen, &mut worklist);
        }

        let mut old_indices_to_scan = std::mem::take(&mut self.remembered);
        old_indices_to_scan.sort_unstable();
        old_indices_to_scan.dedup();
        self.phase = "remembered set (barrera old->young)";
        for packed in old_indices_to_scan {
            self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen, &mut worklist, &mut fixups);
        }

        // Closures/classes/modules hold Rust-side `Value`s the write barrier
        // does not cover; scan only the tracked candidates instead of the
        // whole old gen (which made every minor GC O(old-gen size)).
        let mut candidates = std::mem::take(&mut self.scan_candidates);
        candidates.clear();
        candidates.extend_from_slice(old_gen.scan_roots());
        self.phase = "scan_roots del old gen";
        for &raw_idx in &candidates {
            if matches!(old_gen.get_raw(raw_idx), Some(obj) if Self::can_reference_nursery(obj)) {
                self.scan_and_fix_old_obj(raw_idx, old_gen, &mut worklist, &mut fixups);
            }
        }
        self.scan_candidates = candidates;

        self.phase = "extra roots";
        for &packed in extra_root_packed {
            if is_old_idx(packed) {
                self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen, &mut worklist, &mut fixups);
            } else {
                self.evacuate(packed, old_gen, &mut worklist);
            }
        }

        self.phase = "tareas con valores jóvenes";
        for cell in std::mem::take(&mut old_gen.young_cells) {
            cell.trace_cells(&mut |c| {
                let mut v = c.get();
                self.update_value(&mut v, old_gen, &mut worklist);
                c.set(v);
            });
        }
        for lazy in std::mem::take(&mut old_gen.young_lazies) {
            lazy.trace_cells(&mut |c| {
                let mut v = c.get();
                self.update_value(&mut v, old_gen, &mut worklist);
                c.set(v);
            });
        }

        self.phase = "worklist (hijos de lo ya promovido)";
        while let Some(raw) = worklist.pop() {
            self.scan_and_fix_old_obj(raw, old_gen, &mut worklist, &mut fixups);
        }

        self.objects.clear();
        self.forwarding.clear();
        self.worklist = worklist;
        crate::gc_trace::note_end(trace, self.minor_gc_count, self.minor_gc_promoted);
    }

    #[inline]
    pub(super) fn update_value(
        &mut self,
        val: &mut VmValue,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) {
        if !val.is_heap() {
            return;
        }
        let idx = val.as_heap_idx();
        if !is_nursery_idx(idx) {
            return;
        }
        let packed = self.evacuate(idx, old_gen, worklist);
        *val = VmValue::from_heap_idx(packed);
    }

    /// Como [`Self::update_value`] pero sobre slots REF (`u32` pelados del
    /// frame por clases). Los GPR/FPR ni se visitan: por construcción nunca
    /// son raíces. `REF_UNINIT` (trailing nunca escrito) se salta.
    #[inline]
    pub(super) fn update_ref(
        &mut self,
        slot: &mut u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) {
        if *slot == crate::frame_store::REF_UNINIT {
            return;
        }
        if !is_nursery_idx(*slot) {
            return;
        }
        *slot = self.evacuate(*slot, old_gen, worklist);
    }

    pub(super) fn evacuate(
        &mut self,
        nursery_idx: u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) -> u32 {
        if let Some(Some(fwd)) = self.forwarding.get(nursery_idx as usize) {
            return *fwd;
        }
        let obj = match self.objects.get_mut(nursery_idx as usize) {
            Some(slot @ &mut Some(_)) => slot.take().unwrap(),
            // Llegar aquí significa que algo conserva un índice de nursery que
            // ya no existe: casi siempre una referencia old→young que la
            // barrera de escritura no registró.
            //
            // El resultado tiene que ser un índice VÁLIDO. Devolver uno
            // imposible (`u32::MAX`) para que fallase pronto convierte esto en
            // un SEGFAULT: el código compilado resuelve handles del heap sin
            // comprobar límites, así que un índice fuera de rango revienta el
            // proceso en vez de dar un error de VM. Medido, no supuesto —
            // `bench_http_routing` con 95 000 peticiones lo hace.
            //
            // Así que el valor se queda, pero el suceso deja de ser mudo: sin
            // este aviso el programa sigue con un objeto ajeno en la mano y el
            // fallo aparece mucho después y en otro sitio (un `GetFixedField`
            // sobre la clase `Error`, sin nada que lo ligue a la colección que
            // lo causó). Ver `bench-jit-snapshot-corruption` en las notas del
            // proyecto: la causa raíz sigue abierta.
            _ => {
                debug_assert!(
                    false,
                    "evacuate: índice de nursery {nursery_idx} sin objeto — \
                     referencia old→young no registrada por la barrera"
                );
                // Una vez por proceso: el caso se da dentro de una colección,
                // y una colección puede encontrar miles de referencias al
                // mismo objeto perdido.
                static WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    eprintln!(
                        "warning[gc]: referencia colgante al nursery ({nursery_idx}) durante la \
                         colección menor, fase «{}» — una referencia old→young no quedó \
                         registrada por la barrera de escritura. Los valores leídos a través de \
                         ella serán incorrectos. (Sólo se avisa una vez.)",
                        self.phase
                    );
                }
                return pack_old_idx(0);
            }
        };
        let raw_old = old_gen.alloc_raw(obj);
        let packed = pack_old_idx(raw_old);
        if let Some(slot) = self.forwarding.get_mut(nursery_idx as usize) {
            *slot = Some(packed);
        }
        // Contado aquí, que es donde la promoción ocurre. Derivarlo al final
        // contando entradas en `forwarding` recorría toda la nursery (hasta
        // 49 152 ranuras) en cada colección para una sola estadística.
        self.minor_gc_promoted += 1;
        worklist.push(raw_old);
        packed
    }
}
