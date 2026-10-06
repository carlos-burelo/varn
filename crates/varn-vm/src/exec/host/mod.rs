



mod alloc;
mod buffers;
pub(crate) mod isolates;
mod metadata;
mod objects;
mod sendable;
mod spawn;
mod tasks;

use super::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::{ClassObj, NativeCtx, NativeFn, ResourceStore};

impl NativeCtx for ExecCtx {
    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.heap.alloc_str_dynamic(s)
    }

    fn map_key(
        &mut self,
        v: VmValue,
    ) -> Result<varn_types::value::MapKey, varn_types::NativeError> {
        self.host_map_key(v)
    }

    fn str_map_key(&mut self, s: &str) -> varn_types::value::MapKey {
        self.host_str_map_key(s)
    }

    fn collection_write_barrier(&mut self, parent: VmValue, child: VmValue) {
        if parent.is_heap() {
            self.heap.write_barrier(parent.as_heap(), child);
        }
    }

    fn alloc_str_owned(&mut self, s: String) -> VmValue {
        self.heap.alloc_str_dynamic(&s)
    }

    fn str_repr(&self, v: VmValue) -> String {
        self.heap.str_repr(v)
    }

    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> std::borrow::Cow<'a, str> {
        self.heap.str_repr_borrowed(v)
    }

    fn str_owned(&self, v: VmValue) -> Option<String> {
        self.host_str_owned(v)
    }

    fn str_shared(&self, v: VmValue) -> Option<std::sync::Arc<str>> {
        self.heap.str_shared(v)
    }

    fn str_is_ascii(&self, v: VmValue) -> bool {
        self.heap.str_is_ascii(v)
    }

    fn is_string(&self, v: VmValue) -> bool {
        v.is_sso() || (v.is_heap() && matches!(self.heap.get(v.as_heap()), Some(HeapObj::Str(_))))
    }

    fn is_array(&self, v: VmValue) -> bool {
        v.is_heap()
            && matches!(
                self.heap.get(v.as_heap()),
                Some(HeapObj::Array(_) | HeapObj::Tuple(_))
            )
    }

    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue {
        self.heap.alloc_array_vm(items)
    }

    fn array_len(&self, arr: VmValue) -> usize {
        crate::heap_array::array_len(&self.heap, arr)
    }

    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue> {
        crate::heap_array::array_get(&self.heap, arr, idx)
    }

    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue) {
        crate::heap_array::array_set(&mut self.heap, arr, idx, val)
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        crate::heap_array::array_push(&mut self.heap, arr, val)
    }

    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue> {
        crate::heap_array::array_pop(&self.heap, arr)
    }

    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
        crate::heap_array::array_for_each(&self.heap, arr, f)
    }

    fn is_object(&self, v: VmValue) -> bool {
        self.host_is_object(v)
    }

    fn object_for_each(&self, obj: VmValue, f: &mut dyn FnMut(&str, VmValue)) {
        self.host_object_for_each(obj, f)
    }

    fn map_for_each(&self, map: VmValue, f: &mut dyn FnMut(VmValue, VmValue)) {
        self.host_map_for_each(map, f)
    }

    fn get_object_shape(&self, obj: VmValue) -> Option<std::rc::Rc<varn_types::Shape>> {
        self.host_get_object_shape(obj)
    }

    fn alloc_object(&mut self) -> VmValue {
        self.heap.alloc_object()
    }

    fn alloc_object_with_shape(
        &mut self,
        shape: &std::rc::Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.host_alloc_object_with_shape(shape, values)
    }

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        self.host_get_field(obj, key)
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        self.host_set_field(obj, key, val)
    }

    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        self.heap.alloc_native_fn(f, name)
    }

    fn alloc_class(&mut self, class: std::rc::Rc<ClassObj>) -> VmValue {
        self.heap.alloc_class_vm(class)
    }

    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        self.heap.alloc_range(start, end, inclusive)
    }

    fn alloc_buffer(&mut self, size: usize) -> VmValue {
        self.host_alloc_buffer(size)
    }

    fn alloc_buffer_from_bytes(&mut self, bytes: &[u8]) -> VmValue {
        self.host_alloc_buffer_from_bytes(bytes)
    }

    fn is_buffer(&self, v: VmValue) -> bool {
        self.host_is_buffer(v)
    }

    fn buffer_len(&self, v: VmValue) -> usize {
        self.host_buffer_len(v)
    }

    fn buffer_get_byte(&self, v: VmValue, idx: usize) -> Option<u8> {
        self.host_buffer_get_byte(v, idx)
    }

    fn buffer_set_byte(&mut self, v: VmValue, idx: usize, byte: u8) -> bool {
        self.host_buffer_set_byte(v, idx, byte)
    }

    fn buffer_slice(&mut self, v: VmValue, start: usize, end: usize) -> Option<VmValue> {
        self.host_buffer_slice(v, start, end)
    }

    fn buffer_to_string(&self, v: VmValue) -> Option<String> {
        self.host_buffer_to_string(v)
    }

    fn buffer_to_bytes(&self, v: VmValue) -> Option<Vec<u8>> {
        self.host_buffer_to_bytes(v)
    }

    fn call_vm(
        &mut self,
        callee: VmValue,
        args: &[VmValue],
    ) -> Result<VmValue, varn_types::NativeError> {
        self.host_call_vm(callee, args)
    }

    fn method(&mut self, recv: VmValue, name: &str) -> Option<VmValue> {
        self.bound_method(recv, name)
    }

    fn spawn_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String> {
        self.spawn_internal(callee, args)
    }

    fn suspend_timer(&mut self, ms: u64) -> VmValue {
        varn_builtins::modules::net::driver::driver();
        let promise = varn_runtime::timer::sleep_task(ms);
        self.task_from_host(promise, varn_types::HostOpen::Plain)
    }

    fn task_new(&mut self) -> VmValue {
        crate::task::alloc_handle(&mut self.heap, crate::task::TaskCell::pending())
    }

    fn task_resolved(&mut self, value: VmValue) -> VmValue {
        self.host_task_resolved(value)
    }

    fn task_rejected(&mut self, value: VmValue) -> VmValue {
        self.host_task_rejected(value)
    }

    fn task_from_host(
        &mut self,
        promise: varn_types::HostPromise,
        open: varn_types::HostOpen,
    ) -> VmValue {
        self.host_task_from_host(promise, open)
    }

    fn alloc_bigint(&mut self, value: num_bigint::BigInt) -> VmValue {
        self.heap.alloc_bigint(value)
    }

    fn alloc_decimal(&mut self, value: bigdecimal::BigDecimal) -> VmValue {
        self.heap.alloc_decimal(value)
    }

    fn alloc_char(&mut self, value: char) -> VmValue {
        self.heap.alloc_char(value)
    }

    fn alloc_map(&mut self, entries: Vec<(VmValue, VmValue)>) -> VmValue {
        self.host_alloc_map(entries)
    }

    fn alloc_set(&mut self, items: Vec<VmValue>) -> VmValue {
        self.host_alloc_set(items)
    }

    fn alloc_enum_variant(&mut self, data: varn_types::value::EnumVariantData) -> VmValue {
        self.heap.alloc_enum_variant_vm(data)
    }

    fn alloc_bound_native(
        &mut self,
        receiver: VmValue,
        func: NativeFn,
        name: &'static str,
    ) -> VmValue {
        self.host_alloc_bound_native(receiver, func, name)
    }

    fn task_gather(&mut self, tasks: VmValue) -> Result<VmValue, String> {
        isolates::gather_tasks(self, tasks)
    }

    fn task_yield(&mut self) -> VmValue {
        crate::task::alloc_handle(&mut self.heap, crate::task::TaskCell::yielded())
    }

    fn task_cancel(&mut self, task: VmValue) -> Result<(), String> {
        self.host_task_cancel(task)
    }

    #[allow(clippy::mut_from_ref)]
    fn resources(&mut self) -> &mut ResourceStore {
        unsafe { &mut *self.resources.get() }
    }

    fn as_generator(&self, v: VmValue) -> Option<varn_types::GeneratorObj> {
        self.heap.generator_of(v)
    }

    fn as_char(&self, v: VmValue) -> Option<char> {
        self.heap.char_of(v)
    }

    fn as_bigint(&self, v: VmValue) -> Option<num_bigint::BigInt> {
        self.heap.bigint_of(v)
    }

    fn as_decimal(&self, v: VmValue) -> Option<bigdecimal::BigDecimal> {
        self.heap.decimal_of(v)
    }

    fn as_range(&self, v: VmValue) -> Option<varn_types::value::RangeData> {
        self.heap.range_of(v)
    }

    fn as_map(&self, v: VmValue) -> Option<varn_types::value::MapRef> {
        self.heap.map_of(v)
    }

    fn as_set(&self, v: VmValue) -> Option<varn_types::value::SetRef> {
        self.heap.set_of(v)
    }

    fn is_static_receiver(&self, v: VmValue) -> bool {
        self.heap.is_static_receiver(v)
    }

    fn alloc_range_data(&mut self, range: varn_types::value::RangeData) -> VmValue {
        self.heap.alloc_range_data(range)
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue {
        varn_types::call_static_with(f)
    }

    fn get_class(&self, name: &str) -> Option<std::rc::Rc<ClassObj>> {
        self.heap.get_intrinsic_class(name)
    }

    fn register_class(&mut self, name: &str, cls: std::rc::Rc<ClassObj>) {
        self.heap.set_intrinsic_class(name, cls);
    }

    fn current_source_file(&self) -> Option<String> {
        self.host_current_source_file()
    }

    fn spawn_isolate(
        &mut self,
        module_path: &str,
        export_name: &str,
        args: Vec<varn_types::value::SendValue>,
    ) -> Result<varn_types::HostPromise, String> {
        self.host_spawn_isolate(module_path, export_name, args)
    }

    fn alloc_instance(&mut self, class_name: &str) -> Option<VmValue> {
        self.host_alloc_instance(class_name)
    }

    fn get_function_location(&self, func_val: VmValue) -> Option<(String, String)> {
        self.host_get_function_location(func_val)
    }

    fn load_module(&mut self, specifier: &str) -> Result<VmValue, String> {
        self.load_module(specifier).map_err(|e| format!("{:?}", e))
    }

    fn to_sendable(&self, val: VmValue) -> Result<varn_types::value::SendValue, String> {
        sendable::to_sendable(self, val)
    }

    fn parse_json(&mut self, text: &str) -> Result<VmValue, String> {
        self.json_parse(text)
    }

    fn stringify_json(&mut self, value: VmValue) -> Result<String, String> {
        self.json_stringify(value)
    }

    fn parse_csv(
        &mut self,
        text: &str,
        delimiter: u8,
        has_header: bool,
        trim: bool,
    ) -> Result<VmValue, String> {
        self.host_parse_csv(text, delimiter, has_header, trim)
    }

    fn stringify_csv(&mut self, value: VmValue, delimiter: u8) -> Result<String, String> {
        crate::exec::ctx_csv::stringify_csv(self, value, delimiter)
    }

    fn capabilities(&self) -> &varn_types::capabilities::CapabilitySet {
        &self.capabilities
    }

    fn define_metadata(&mut self, target: VmValue, key: &str, value: VmValue) {
        self.host_define_metadata(target, key, value)
    }

    fn get_metadata(&self, target: VmValue, key: &str) -> Option<VmValue> {
        self.host_get_metadata(target, key)
    }

    fn has_metadata(&self, target: VmValue, key: &str) -> bool {
        self.host_has_metadata(target, key)
    }
}
