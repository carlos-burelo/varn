use varn_types::{VmArray, VmValue};

#[test]
fn typed_i8_array_reads_back_as_int_vmvalue() {
    let a = VmArray::new_i8(vec![-128, 0, 127]);
    assert_eq!(a.discriminant(), 3);
    assert_eq!(a.len(), 3);
    assert_eq!(a.get_vm(0), Some(VmValue::from_int(-128)));
    assert_eq!(a.get_vm(2), Some(VmValue::from_int(127)));
}

#[test]
fn typed_u32_array_reads_back_unsigned() {
    let a = VmArray::new_u32(vec![0, 4_000_000_000]);
    assert_eq!(a.get_vm(1), Some(VmValue::from_int(4_000_000_000)));
}

#[test]
fn typed_f32_array_reads_back_as_widened_float() {
    let a = VmArray::new_f32(vec![1.5, -2.25]);
    assert_eq!(a.get_vm(0), Some(VmValue::from_f64(1.5)));
}

#[test]
fn narrow_variants_have_distinct_discriminants() {
    let discs = [
        VmArray::new_i8(vec![]).discriminant(),
        VmArray::new_i16(vec![]).discriminant(),
        VmArray::new_i32(vec![]).discriminant(),
        VmArray::new_u8(vec![]).discriminant(),
        VmArray::new_u16(vec![]).discriminant(),
        VmArray::new_u32(vec![]).discriminant(),
        VmArray::new_f32(vec![]).discriminant(),
    ];
    let mut sorted = discs.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 7, "discriminants must be pairwise distinct");
}
