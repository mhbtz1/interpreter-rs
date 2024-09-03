use std::ptr::{NonNull, write};
use rand::Rng;

pub type BlockPtr = NonNull<u8>;
pub type BlockSize = usize;
pub const BLOCK_SIZE_BITS: usize = 15;
pub const BLOCK_SIZE: usize = 1 << BLOCK_SIZE_BITS;
pub const LINE_SIZE_BITS: usize = 7;
pub const LINE_SIZE: usize = 1 << LINE_SIZE_BITS;
pub const LINE_COUNT: usize = BLOCK_SIZE / LINE_SIZE;
pub const BLOCK_CAPACITY: usize = BLOCK_SIZE - LINE_COUNT;

pub struct Block {
    pub ptr: BlockPtr,
    pub size: BlockSize
}

pub struct BlockMeta {
    lines: *mut u8
}

// interface for bump allocation
pub struct BumpBlock {
    pub cursor: *const u8, //next address for allocating a new block
    pub limit: *const u8,
    pub block: Block, // block to allocate
    pub lines: *mut u8 //address to marked line
}

impl BumpBlock {

    pub fn inner_alloc(&mut self, alloc_size: usize) -> Option<*const u8> {
        let ptr = self.cursor as usize;
        let limit = self.limit as usize;

        let next_ptr = ptr.checked_sub(alloc_size)? & constants::ALLOC_ALIGN_MASK;

        if next_ptr < limit {
            let block_relative_limit =
                unsafe { self.limit.sub(self.block.as_ptr() as usize) } as usize;

            if block_relative_limit > 0 {
                if let Some((cursor, limit)) = self
                    .meta
                    .find_next_available_hole(block_relative_limit, alloc_size)
                {
                    self.cursor = unsafe { self.block.as_ptr().add(cursor) };
                    self.limit = unsafe { self.block.as_ptr().add(limit) };
                    return self.inner_alloc(alloc_size);
                }
            }

            None
        } else {
            self.cursor = next_ptr as *const u8;
            Some(self.cursor)
        }
    }


    pub fn find_next_available_hole(
        &self,
        starting_at: usize,
        alloc_size: usize,
    ) -> Option<(usize, usize)> {
        // The count of consecutive avaliable holes. Must take into account a conservatively marked
        // hole at the beginning of the sequence.
        let mut count = 0;
        let starting_line = starting_at / constants::LINE_SIZE; //the index of the starting line in the block
        let lines_required = (alloc_size + constants::LINE_SIZE - 1) / constants::LINE_SIZE; //number of lines in block necessary to allocate object of "alloc_size" bytes
        // Counting down from the given search start index
        let mut end = starting_line;

        for index in (0..starting_line).rev() {
            let marked = unsafe { *self.lines.add(index) };

            if marked == 0 {
                // count unmarked lines
                count += 1;

                if index == 0 && count >= lines_required {
                    let limit = index * constants::LINE_SIZE;
                    let cursor = end * constants::LINE_SIZE;
                    return Some((cursor, limit));
                }
            } else {
                // This block is marked
                if count > lines_required {
                    // But at least 2 previous blocks were not marked. Return the hole, considering the
                    // immediately preceding block as conservatively marked
                    let limit = (index + 2) * constants::LINE_SIZE;
                    let cursor = end * constants::LINE_SIZE;
                    return Some((cursor, limit));
                }

                // If this line is marked and we didn't return a new cursor/limit pair by now,
                // reset the hole search state
                count = 0;
                end = index;
            }
        }

        None
    }

    unsafe fn write<T>(dest: *const u8, object: T) {
        write(dest as *mut T, object);
    }
}


// simple API for a memory allocator in Rust
#[derive(Debug, PartialEq)]
impl Block {
    pub fn new(size: BlockSize) -> Result<Block, std::err::Err> {
        if !size.is_power_of_two() {
            return Err("Size is not a power of two!")
        }
        return Ok(Block { ptr: internal::alloc_block(size), size: size})
    }

    pub fn alloc_block(size: BlockSize) -> Result<BlockPtr, std::err::Err> {
        unsafe {
            std::alloc::Layout::from_size_align_unchecked(size, size);
            let ptr = alloc(layout);
            if ptr.is_null() {
                Err("no memory!")
            } else {
                Ok(NonNull::new_unchecked(ptr)) 
            }
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr();
    }

    pub fn dealloc_block(block: Block) -> Result<()> {
        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(block.size, block.size);
            dealloc(block.ptr.as_ptr())
        }
    }


}


fn main() {
    let addr = rand::thread_rng().gen();
    let block = BumpBlock { cursor: addr as *const u8, limit: , block: Block::new(128), meta: }
    let allocated_addr = block.inner_alloc(128).unwrap();
}
