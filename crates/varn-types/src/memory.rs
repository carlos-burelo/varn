use std::cell::Cell;

thread_local! {
    static GROWN: Cell<u64> = const { Cell::new(0) };
    static BUDGET: Cell<u64> = const { Cell::new(u64::MAX) };
    static LIMIT: Cell<*mut u64> = const { Cell::new(std::ptr::null_mut()) };
}

pub struct ExternalLedger;

impl ExternalLedger {
    pub fn attach(limit: *mut u64, budget: u64) {
        GROWN.with(|c| c.set(0));
        BUDGET.with(|c| c.set(budget));
        LIMIT.with(|c| c.set(limit));
    }

    pub fn detach() {
        LIMIT.with(|c| c.set(std::ptr::null_mut()));
        BUDGET.with(|c| c.set(u64::MAX));
    }

    pub fn set_budget(budget: u64) {
        BUDGET.with(|c| c.set(budget));
    }

    #[inline(always)]
    pub fn grow(bytes: usize) {
        if bytes == 0 {
            return;
        }
        let total = GROWN.with(|c| {
            let total = c.get().saturating_add(bytes as u64);
            c.set(total);
            total
        });
        if total >= BUDGET.with(Cell::get) {
            let limit = LIMIT.with(Cell::get);
            if !limit.is_null() {
                unsafe { std::ptr::write(limit, 0) };
            }
        }
    }

    #[inline(always)]
    pub fn total() -> u64 {
        GROWN.with(Cell::get)
    }

    pub fn reset() {
        GROWN.with(|c| c.set(0));
    }
}
