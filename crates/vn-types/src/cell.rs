pub const BLOCK_BYTES: usize = 256 * 1024;
pub const CELL_ALIGN: usize = 16;

pub const MAX_MARK_WORDS: usize = BLOCK_BYTES / 16 / 64;
pub const BLOCK_HEAD_BYTES: usize = 2080;

pub const CELL_CLASSES: [usize; 24] = [
    16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768,
    1024, 1536, 2048,
];

pub fn class_for(bytes: usize) -> Option<usize> {
    CELL_CLASSES.iter().position(|&c| c >= bytes)
}

pub fn cells_per_block(cell: usize) -> usize {
    (BLOCK_BYTES - BLOCK_HEAD_BYTES) / cell
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

pub const CELL_KIND_INSTANCE: u8 = 0xFF;
pub const STATE_NEEDS_DROP: u8 = 0x10;
pub const STATE_SIDE_TABLE: u8 = 0x08;

pub const INST_CELL_DATA_OFF: usize = HEADER_BYTES;

pub fn instance_colocated_body_bytes(payload_size: u32) -> usize {
    8 + (payload_size as usize).div_ceil(8) * 8
}

pub fn instance_bytes_for(payload_size: u32) -> usize {
    crate::INST_PAYLOAD_OFF + (payload_size as usize).div_ceil(8) * 8
}

pub const LANE_FREE_OFF: usize = 0;
pub const LANE_BUMP_OFF: usize = 8;
pub const LANE_END_OFF: usize = 16;
pub const LANE_SIZE: usize = 24;

pub const SIZE_CLASS_BLOCKS_OFF: usize = 0;
pub const SIZE_CLASS_LANE_OFF: usize = 24;
pub const SIZE_CLASS_STRIDE: usize = 56;

pub const VEC_CAP_OFF: usize = 0;
pub const VEC_PTR_OFF: usize = 8;
pub const VEC_LEN_OFF: usize = 16;
