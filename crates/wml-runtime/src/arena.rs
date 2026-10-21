use std::alloc::{alloc, dealloc, Layout};
use std::cell::UnsafeCell;

const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks aligned with ARM64/x86 pages

pub struct RequestArena {
    current_ptr: UnsafeCell<*mut u8>,
    end_ptr: UnsafeCell<*mut u8>,
    base_ptr: *mut u8,
}

impl RequestArena {
    pub fn new() -> Self {
        let layout = Layout::from_size_align(CHUNK_SIZE, 64).expect("Invalid layout");
        let base_ptr = unsafe { alloc(layout) };
        assert!(!base_ptr.is_null(), "Failed to allocate RequestArena slab");

        let end_ptr = unsafe { base_ptr.add(CHUNK_SIZE) };

        Self {
            current_ptr: UnsafeCell::new(base_ptr),
            end_ptr: UnsafeCell::new(end_ptr),
            base_ptr,
        }
    }

    /// Fast bump allocation: ~2 CPU instructions without system malloc locks
    #[inline(always)]
    pub fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        let current = unsafe { *self.current_ptr.get() };
        let aligned = (current as usize + (align - 1)) & !(align - 1);
        let next = aligned + size;

        if next <= unsafe { *self.end_ptr.get() } as usize {
            unsafe {
                *self.current_ptr.get() = next as *mut u8;
            }
            aligned as *mut u8
        } else {
            // Oversized or overflow allocation fallback
            self.alloc_overflow(size, align)
        }
    }

    #[cold]
    fn alloc_overflow(&self, size: usize, align: usize) -> *mut u8 {
        let layout = Layout::from_size_align(size, align).unwrap();
        unsafe { alloc(layout) }
    }

    /// Zero-cost teardown: O(1) pointer reset when HTTP response ends
    #[inline(always)]
    pub fn reset(&self) {
        unsafe {
            *self.current_ptr.get() = self.base_ptr;
        }
    }
}

impl Drop for RequestArena {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(CHUNK_SIZE, 64).expect("Invalid layout");
        unsafe {
            dealloc(self.base_ptr, layout);
        }
    }
}

unsafe impl Send for RequestArena {}
unsafe impl Sync for RequestArena {}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arena_bump_allocation_and_reset() {
        let arena = RequestArena::new();

        let ptr1 = arena.alloc(128, 8);
        assert!(!ptr1.is_null());

        let ptr2 = arena.alloc(256, 8);
        assert!(!ptr2.is_null());
        assert_eq!(ptr2 as usize, ptr1 as usize + 128);

        // Reset must be O(1) and reset pointer back to base
        arena.reset();
        let ptr3 = arena.alloc(128, 8);
        assert_eq!(ptr3, ptr1);
    }
}
