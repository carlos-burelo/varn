use std::time::Instant;
use varn_types::value::{MapKey, ObjRef, ValueMap};
use varn_types::vm_value::VmValue;

#[derive(Clone)]
struct InlineMap<const CAP: usize> {
    len: u8,
    entries: [(MapKey, VmValue); CAP],
}

impl<const CAP: usize> Default for InlineMap<CAP> {
    #[inline(always)]
    fn default() -> Self {
        Self {
            len: 0,
            entries: [(MapKey(VmValue::null()), VmValue::null()); CAP],
        }
    }
}

impl<const CAP: usize> InlineMap<CAP> {
    #[inline(always)]
    fn insert(&mut self, key: MapKey, val: VmValue) {
        let n = self.len as usize;
        for i in 0..n {
            if self.entries[i].0 == key {
                self.entries[i].1 = val;
                return;
            }
        }
        if n < CAP {
            self.entries[n] = (key, val);
            self.len += 1;
        }
    }

    #[inline(always)]
    fn get(&self, key: MapKey) -> Option<VmValue> {
        let n = self.len as usize;
        for i in 0..n {
            if self.entries[i].0 == key {
                return Some(self.entries[i].1);
            }
        }
        None
    }
}

#[test]
fn bench_phase0_map_representations() {
    const ITERS: usize = 100_000;

    let k_id = MapKey(VmValue::try_from_sso("id").unwrap());
    let k_user = MapKey(VmValue::try_from_sso("user").unwrap());
    let k_action = MapKey(VmValue::try_from_sso("act").unwrap());
    let val1 = VmValue::from_i32(101);
    let val2 = VmValue::from_i32(202);
    let val3 = VmValue::from_i32(303);

    // Warm-up
    {
        let mut map = ValueMap::default();
        map.insert(k_id, val1);
        assert_eq!(map.get(&k_id), Some(&val1));
    }

    // 1. Camino Actual: Shape + ObjRef
    let t0 = Instant::now();
    let mut sum_shape = 0i64;
    for _ in 0..ITERS {
        let obj = ObjRef::empty();
        obj.set_field_str("id", val1);
        obj.set_field_str("user", val2);
        obj.set_field_str("act", val3);
        let r1 = obj.get_field_nv("id").unwrap();
        let r2 = obj.get_field_nv("user").unwrap();
        let r3 = obj.get_field_nv("act").unwrap();
        sum_shape += std::hint::black_box(r1.as_int() + r2.as_int() + r3.as_int());
    }
    let elapsed_shape = t0.elapsed();

    // 2. FxHashMap (ValueMap)
    let t0 = Instant::now();
    let mut sum_fx = 0i64;
    for _ in 0..ITERS {
        let mut map = ValueMap::default();
        map.insert(k_id, val1);
        map.insert(k_user, val2);
        map.insert(k_action, val3);
        let r1 = *map.get(&k_id).unwrap();
        let r2 = *map.get(&k_user).unwrap();
        let r3 = *map.get(&k_action).unwrap();
        sum_fx += std::hint::black_box(r1.as_int() + r2.as_int() + r3.as_int());
    }
    let elapsed_fx = t0.elapsed();

    // 3. Inline Array (Linear Scan, N=4)
    let t0 = Instant::now();
    let mut sum_inline = 0i64;
    for _ in 0..ITERS {
        let mut map = InlineMap::<4>::default();
        map.insert(k_id, val1);
        map.insert(k_user, val2);
        map.insert(k_action, val3);
        let r1 = map.get(k_id).unwrap();
        let r2 = map.get(k_user).unwrap();
        let r3 = map.get(k_action).unwrap();
        sum_inline += std::hint::black_box(r1.as_int() + r2.as_int() + r3.as_int());
    }
    let elapsed_inline = t0.elapsed();

    // 4. Rc<RefCell<InlineMap<4>>> (Heap alocado, inline hasta 4)
    let t0 = Instant::now();
    let mut sum_heap = 0i64;
    for _ in 0..ITERS {
        let map = std::rc::Rc::new(std::cell::RefCell::new(InlineMap::<4>::default()));
        {
            let mut m = map.borrow_mut();
            m.insert(k_id, val1);
            m.insert(k_user, val2);
            m.insert(k_action, val3);
        }
        let m = map.borrow();
        let r1 = m.get(k_id).unwrap();
        let r2 = m.get(k_user).unwrap();
        let r3 = m.get(k_action).unwrap();
        sum_heap += std::hint::black_box(r1.as_int() + r2.as_int() + r3.as_int());
    }
    let elapsed_heap = t0.elapsed();

    // 5. Rc<RefCell<InlineMap<8>>> (Heap alocado, inline hasta 8)
    let t0 = Instant::now();
    let mut sum_heap8 = 0i64;
    for _ in 0..ITERS {
        let map = std::rc::Rc::new(std::cell::RefCell::new(InlineMap::<8>::default()));
        {
            let mut m = map.borrow_mut();
            m.insert(k_id, val1);
            m.insert(k_user, val2);
            m.insert(k_action, val3);
        }
        let m = map.borrow();
        let r1 = m.get(k_id).unwrap();
        let r2 = m.get(k_user).unwrap();
        let r3 = m.get(k_action).unwrap();
        sum_heap8 += std::hint::black_box(r1.as_int() + r2.as_int() + r3.as_int());
    }
    let elapsed_heap8 = t0.elapsed();

    assert_eq!(sum_shape, sum_fx);
    assert_eq!(sum_shape, sum_inline);
    assert_eq!(sum_shape, sum_heap);
    assert_eq!(sum_shape, sum_heap8);

    println!("\n=======================================================");
    println!(
        "FASE 0: RESULTADOS DEL MICRO-BENCHMARK ({} iters, 3 claves)",
        ITERS
    );
    println!("-------------------------------------------------------");
    println!(
        "1. Shape + ObjData (actual)    : {:>8.2?} ({:.1} ns/op)",
        elapsed_shape,
        (elapsed_shape.as_nanos() as f64) / (ITERS as f64)
    );
    println!(
        "2. FxHashMap<MapKey, VmVal>    : {:>8.2?} ({:.1} ns/op)",
        elapsed_fx,
        (elapsed_fx.as_nanos() as f64) / (ITERS as f64)
    );
    println!(
        "3. InlineMap<4> (stack puro)   : {:>8.2?} ({:.1} ns/op)",
        elapsed_inline,
        (elapsed_inline.as_nanos() as f64) / (ITERS as f64)
    );
    println!(
        "4. Rc<RefCell<InlineMap<4>>>   : {:>8.2?} ({:.1} ns/op)",
        elapsed_heap,
        (elapsed_heap.as_nanos() as f64) / (ITERS as f64)
    );
    println!(
        "5. Rc<RefCell<InlineMap<8>>>   : {:>8.2?} ({:.1} ns/op)",
        elapsed_heap8,
        (elapsed_heap8.as_nanos() as f64) / (ITERS as f64)
    );
    println!("-------------------------------------------------------");
    let speedup_heap = elapsed_shape.as_secs_f64() / elapsed_heap.as_secs_f64();
    println!(
        "Speedup Rc<InlineMap<4>> vs Shape: {:.2}x más rápido",
        speedup_heap
    );
    let speedup_heap8 = elapsed_shape.as_secs_f64() / elapsed_heap8.as_secs_f64();
    println!(
        "Speedup Rc<InlineMap<8>> vs Shape: {:.2}x más rápido",
        speedup_heap8
    );
    println!("=======================================================\n");

    // 7 keys benchmark (mimicking CSV ETL row: id, cust, cat, amt, tax, stat, date)
    let k4 = MapKey(VmValue::try_from_sso("k4").unwrap());
    let k5 = MapKey(VmValue::try_from_sso("k5").unwrap());
    let k6 = MapKey(VmValue::try_from_sso("k6").unwrap());
    let k7 = MapKey(VmValue::try_from_sso("k7").unwrap());

    let t0 = Instant::now();
    for _ in 0..ITERS {
        let mut map = ValueMap::default();
        map.insert(k_id, val1);
        map.insert(k_user, val2);
        map.insert(k_action, val3);
        map.insert(k4, val1);
        map.insert(k5, val2);
        map.insert(k6, val3);
        map.insert(k7, val1);
        let _ = std::hint::black_box(map.get(&k7));
    }
    let el_fx_7 = t0.elapsed();

    let t0 = Instant::now();
    for _ in 0..ITERS {
        let map = std::rc::Rc::new(std::cell::RefCell::new(InlineMap::<8>::default()));
        {
            let mut m = map.borrow_mut();
            m.insert(k_id, val1);
            m.insert(k_user, val2);
            m.insert(k_action, val3);
            m.insert(k4, val1);
            m.insert(k5, val2);
            m.insert(k6, val3);
            m.insert(k7, val1);
        }
        let m = map.borrow();
        let _ = std::hint::black_box(m.get(k7));
    }
    let el_inline8_7 = t0.elapsed();

    println!("7 CLAVES (CSV ETL ROW):");
    println!(
        "FxHashMap (7 keys)          : {:>8.2?} ({:.1} ns/op)",
        el_fx_7,
        (el_fx_7.as_nanos() as f64) / (ITERS as f64)
    );
    println!(
        "Rc<InlineMap<8>> (7 keys)   : {:>8.2?} ({:.1} ns/op)",
        el_inline8_7,
        (el_inline8_7.as_nanos() as f64) / (ITERS as f64)
    );
    println!(
        "Speedup Rc<InlineMap<8>> (7k): {:.2}x más rápido",
        el_fx_7.as_secs_f64() / el_inline8_7.as_secs_f64()
    );
    println!("=======================================================\n");
}
