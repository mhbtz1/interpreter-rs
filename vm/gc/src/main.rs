use std::ptr::NonNull;

pub type BlockPtr = NonNull<u8>;
pub type BlockSize = usize;
pub struct Block {
    ptr: BlockPtr,
    size: BlockSize
}

impl Block {
    pub fn new(size: BlockSize) -> Result<Block, std::err::Err> {
        if !size.is_power_of_two() {
            return Err("Size is not a power of two!")
        }
        return Ok(Block { ptr: internal::alloc_block(size), size: size})
    }
}


fn main() {
    println!("Hello, world!");
}
