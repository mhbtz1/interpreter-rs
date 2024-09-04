mod alloc_api;

use std::{marker::PhantomData, ptr::{write, NonNull}};
use alloc_api::{SizeClass, AllocRaw, AllocHeader, AllocObject, AllocTypeId, BlockError, RawPtr};
use core::cell::UnsafeCell;

pub type BlockPtr = NonNull<u8>;
pub type BlockSize = usize;
pub const BLOCK_SIZE_BITS: usize = 15;
pub const BLOCK_SIZE: usize = 1 << BLOCK_SIZE_BITS;
pub const LINE_SIZE_BITS: usize = 7;
pub const LINE_SIZE: usize = 1 << LINE_SIZE_BITS;
pub const LINE_COUNT: usize = BLOCK_SIZE / LINE_SIZE;
pub const BLOCK_CAPACITY: usize = BLOCK_SIZE - LINE_COUNT;


pub struct AllocError {

}

pub struct Block {
    pub ptr: BlockPtr,
    pub size: BlockSize
}

pub struct BlockMeta {
    lines: *mut u8
}

impl BlockMeta {
    pub fn find_next_available_hole(
        &self,
        starting_at: usize,
        alloc_size: usize,
    ) -> Option<(usize, usize)> {
        // The count of consecutive avaliable holes. Must take into account a conservatively marked
        // hole at the beginning of the sequence.
        let mut count = 0;
        let starting_line = starting_at / LINE_SIZE; //the index of the starting line in the block
        let lines_required = (alloc_size + LINE_SIZE - 1) / LINE_SIZE; //number of lines in block necessary to allocate object of "alloc_size" bytes
        // Counting down from the given search start index
        let mut end = starting_line;

        for index in (0..starting_line).rev() {
            let marked = unsafe { *self.lines.add(index) };

            if marked == 0 {
                // count unmarked lines
                count += 1;

                if index == 0 && count >= lines_required {
                    let limit = index * LINE_SIZE;
                    let cursor = end * LINE_SIZE;
                    return Some((cursor, limit));
                }
            } else {
                // This block is marked
                if count > lines_required {
                    // But at least 2 previous blocks were not marked. Return the hole, considering the
                    // immediately preceding block as conservatively marked
                    let limit = (index + 2) * LINE_SIZE;
                    let cursor = end * LINE_SIZE;
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
}

// interface for bump allocation
pub struct BumpBlock {
    pub cursor: *const u8, // ending index of line in the block where the last object was written
    pub limit: *const u8, // starting index of line in the block where the last object was written
    pub block: Block, // information about current block
    pub meta: BlockMeta //address to line containing marked information (i.e. lines that contain allocated objects)
}

pub struct BlockList {
    head: Option<BumpBlock>,
    overflow: Option<BumpBlock>,
    list: Vec<BumpBlock>,
}

pub struct StickyImmixHeap<H> {
    blocks: UnsafeCell<BlockList>,
    _header_type: PhantomData<*const H>
}

impl StickyImmixHeap<H> {
    fn find_space(
        &self,
        alloc_size: usize,
        size_class: SizeClass,
    ) -> Result<*const u8, AllocError> {
        let blocks = unsafe { &mut *self.blocks.get() };
        match blocks.head {
            Some(ref mut head) => {
                if size_class == SizeClass::Medium && alloc_size > head.current_hole_size() {
                    return blocks.overflow_alloc(alloc_size)
                } else {
                    return Ok(head.inner_alloc(alloc_size).unwrap())
                }
            }
            None => {
                blocks.head = Some(blocks.overflow_alloc(alloc_size));
            }
        }
    }
}

impl <H: AllocHeader> AllocRaw for StickyImmixHeap<H> {
    type Header = H
    fn alloc<T>(&self, object: T) -> Result<RawPtr<T>, AllocError>
    where
        T: AllocObject<<Self::Header as AllocHeader>::TypeId>,
    {
        // calculate the total size of the object and it's header
        let header_size = size_of::<Self::Header>();
        let object_size = size_of::<T>();
        let total_size = header_size + object_size;

        // round the size to the next word boundary to keep objects aligned and get the size class
        // TODO BUG? should this be done separately for header and object?
        //  If the base allocation address is where the header gets placed, perhaps
        //  this breaks the double-word alignment object alignment desire?
        let alloc_size = alloc_size_of(total_size);
        let size_class = SizeClass::get_for_size(alloc_size)?;

        // attempt to allocate enough space for the header and the object
        let space = self.find_space(alloc_size, size_class)?;

        // instantiate an object header for type T, setting the mark bit to "allocated"
        let header = Self::Header::new::<T>(object_size as ArraySize, size_class, Mark::Allocated);

        // write the header into the front of the allocated space
        unsafe {
            write(space as *mut Self::Header, header);
        }

        // write the object into the allocated space after the header
        let object_space = unsafe { space.offset(header_size as isize) };
        unsafe {
            write(object_space as *mut T, object);
        }

        // return a pointer to the object in the allocated space
        Ok(RawPtr::new(object_space as *const T))
    }

    fn alloc_array(&self, size_bytes: ArraySize) -> Result<RawPtr<u8>, AllocError> {
        // calculate the total size of the array and it's header
        let header_size = size_of::<Self::Header>();
        let total_size = header_size + size_bytes as usize;

        // round the size to the next word boundary to keep objects aligned and get the size class
        let alloc_size = std::alloc::alloc_size_of(total_size);
        let size_class = SizeClass::get_for_size(alloc_size)?;

        // attempt to allocate enough space for the header and the array
        let space = self.find_space(alloc_size, size_class)?;

        // instantiate an object header for an array, setting the mark bit to "allocated"
        let header = Self::Header::new_array(size_bytes, size_class, Mark::Allocated);

        // write the header into the front of the allocated space
        unsafe {
            write(space as *mut Self::Header, header);
        }

        // calculate where the array will begin after the header
        let array_space = unsafe { space.offset(header_size as isize) };

        // Initialize object_space to zero here.
        // If using the system allocator for any objects (SizeClass::Large, for example),
        // the memory may already be zeroed.
        let array = unsafe { from_raw_parts_mut(array_space as *mut u8, size_bytes as usize) };
        // The compiler should recognize this as optimizable
        for byte in array {
            *byte = 0;
        }

        // return a pointer to the array in the allocated space
        Ok(RawPtr::new(array_space as *const u8))
    }

    fn get_header(object: NonNull<()>) -> NonNull<Self::Header> {
        unsafe { NonNull::new_unchecked(object.cast::<Self::Header>().as_ptr().offset(-1)) }
    }
    fn get_object(header: NonNull<Self::Header>) -> NonNull<()> {
        unsafe { NonNull::new_unchecked(header.as_ptr().offset(1).cast::<()>()) }
    }
}

impl BlockList {
    fn overflow_alloc(&mut self, alloc_size: usize) -> Result<*const u8, AllocError> {
        match self.overflow {
            Some(ref mut overflow) => {
                match overflow.inner_alloc(alloc_size) {
                    Some(space) => Ok(space),
                    None => {
                        let previous = unsafe { std::ptr::replace(overflow, BumpBlock::new()?) };
                        self.rest.push(previous);
                        Ok(overflow.inner_alloc(alloc_size).expect("Not enough space to allocate memory!")) //recursively allocates blocks
                    }
                }
            }
            None => {
                let mut overflow = BumpBlock::new()?;
                let allocated_mem = overflow.inner_alloc(alloc_size).expect("Not enough space to allocate memory!"); //recursively allocates blocks
                self.overflow = Some(overflow);
                allocated_mem //address for 
            }
        }
    }
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
                    return self.inner_alloc(alloc_size); //recursive call
                }
            }

            None
        } else {
            self.cursor = next_ptr as *const u8;
            Some(self.cursor)
        }
    }

}


// simple API for a memory allocator in Rust
impl Block {
    pub fn new(size: BlockSize) -> Result<Block, BlockError> {
        if !size.is_power_of_two() {
            return Err(BlockError::BadRequest);
        }
        return Ok(Block { ptr: std::internal::alloc_block(size), size: size})
    }

    pub fn alloc_block(size: BlockSize) -> Result<BlockPtr, BlockError> {
        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(size, size);
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                Err(BlockError::BadRequest)
            } else {
                Ok(NonNull::new_unchecked(ptr)) 
            }
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    pub fn dealloc_block(block: Block) {
        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(block.size, block.size);
            std::alloc::dealloc(block.ptr.as_ptr(), layout);
        }
    }

}