pub const BLOCK_BYTES: usize = 256 * 1024;
pub const CELL_ALIGN: usize = 16;

pub const CELL_CLASSES: [usize; 24] = [
    16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768,
    1024, 1536, 2048,
];

pub fn class_for(bytes: usize) -> Option<usize> {
    CELL_CLASSES.iter().position(|&c| c >= bytes)
}

pub fn cells_per_block(cell: usize) -> usize {
    BLOCK_BYTES / cell
}

pub const HEADER_BYTES: usize = 8;
pub const HEADER_STATE_OFF: usize = 0;
pub const HEADER_KIND_OFF: usize = 1;
pub const HEADER_CLASS_OFF: usize = 2;

pub const SLOT_STATE_FREE: u8 = 0;
pub const SLOT_STATE_OLD: u8 = 1;
pub const SLOT_STATE_REMEMBERED: u8 = 2;
pub const SLOT_STATE_YOUNG: u8 = 3;
pub const SLOT_STATE_MARKED: u8 = 4;
pub const CELL_CLASS_LARGE: u8 = u8::MAX;

pub const INST_CLASS_ID_OFF: usize = 0;
pub const INST_PAYLOAD_OFF: usize = 8;

pub const fn instance_data_off(heap_obj_bytes: usize) -> usize {
    HEADER_BYTES + heap_obj_bytes
}

pub fn instance_body_bytes(heap_obj_bytes: usize, payload_size: u32) -> usize {
    heap_obj_bytes + instance_bytes_for(payload_size)
}

pub fn instance_bytes_for(payload_size: u32) -> usize {
    INST_PAYLOAD_OFF + (payload_size as usize).div_ceil(8) * 8
}

pub fn instance_cell_bytes(heap_obj_bytes: usize, payload_size: u32) -> usize {
    HEADER_BYTES + instance_body_bytes(heap_obj_bytes, payload_size)
}

pub const LANE_FREE_OFF: usize = 0;
pub const LANE_BUMP_OFF: usize = 8;
pub const LANE_END_OFF: usize = 16;
pub const LANE_SIZE: usize = 24;

pub const SIZE_CLASS_BLOCKS_OFF: usize = 0;
pub const SIZE_CLASS_LANE_OFF: usize = 24;
pub const SIZE_CLASS_STRIDE: usize = 48;

pub const VEC_CAP_OFF: usize = 0;
pub const VEC_PTR_OFF: usize = 8;
pub const VEC_LEN_OFF: usize = 16;

#[cfg(test)]
mod tests {
    use super::{VEC_CAP_OFF, VEC_LEN_OFF, VEC_PTR_OFF};

    #[test]
    fn vec_triple_matches_std_layout() {
        let mut v: Vec<u64> = Vec::with_capacity(16);
        v.push(0xAA);
        let base = &v as *const _ as *const usize;
        unsafe {
            assert_eq!(*base.add(0), v.capacity());
            assert_eq!(*base.add(1), v.as_ptr() as usize);
            assert_eq!(*base.add(2), v.len());
            assert_eq!(VEC_CAP_OFF, 0);
            assert_eq!(VEC_PTR_OFF, 8);
            assert_eq!(VEC_LEN_OFF, 16);
        }
    }
}
