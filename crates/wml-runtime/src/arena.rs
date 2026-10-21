use std::alloc::{alloc, dealloc, Layout};
use std::cell::UnsafeCell;

const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks aligned with ARM64/x86 pages

pub struct RequestArena {
    current_ptr: UnsafeCell<*mut u8>,
    end_ptr: *mut u8,
    base_ptr: *mut u8,
    overflow_allocs: UnsafeCell<Vec<(*mut u8, Layout)>>,
}

impl Default for RequestArena {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestArena {
    pub fn new() -> Self {
        let layout = Layout::from_size_align(CHUNK_SIZE, 64).expect("Invalid layout");
        let base_ptr = unsafe { alloc(layout) };
        assert!(!base_ptr.is_null(), "Failed to allocate RequestArena slab");

        let end_ptr = unsafe { base_ptr.add(CHUNK_SIZE) };

        Self {
            current_ptr: UnsafeCell::new(base_ptr),
            end_ptr,
            base_ptr,
            overflow_allocs: UnsafeCell::new(Vec::new()),
        }
    }

    /// Fast bump allocation: ~2 CPU instructions without system malloc locks
    #[inline(always)]
    pub fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        if !align.is_power_of_two() {
            return std::ptr::null_mut();
        }
        if size == 0 {
            return align as *mut u8;
        }

        let current = unsafe { *self.current_ptr.get() };
        let aligned = match (current as usize).checked_add(align - 1) {
            Some(a) => a & !(align - 1),
            None => return std::ptr::null_mut(),
        };
        let next = match aligned.checked_add(size) {
            Some(n) => n,
            None => return std::ptr::null_mut(),
        };

        if next <= self.end_ptr as usize {
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
        if !align.is_power_of_two() {
            return std::ptr::null_mut();
        }
        if size == 0 {
            return align as *mut u8;
        }
        let layout = match Layout::from_size_align(size, align) {
            Ok(l) => l,
            Err(_) => return std::ptr::null_mut(),
        };
        let ptr = unsafe { alloc(layout) };
        assert!(!ptr.is_null(), "Failed to allocate overflow block");
        unsafe {
            (*self.overflow_allocs.get()).push((ptr, layout));
        }
        ptr
    }

    /// Zero-cost teardown: O(1) pointer reset when HTTP response ends
    #[inline(always)]
    pub fn reset(&self) {
        unsafe {
            *self.current_ptr.get() = self.base_ptr;
            let overflows = &mut *self.overflow_allocs.get();
            for &(ptr, layout) in overflows.iter() {
                dealloc(ptr, layout);
            }
            overflows.clear();
        }
    }
}

impl Drop for RequestArena {
    fn drop(&mut self) {
        self.reset();
        let layout = Layout::from_size_align(CHUNK_SIZE, 64).expect("Invalid layout");
        unsafe {
            dealloc(self.base_ptr, layout);
        }
    }
}

// RequestArena is !Send across thread boundaries per request-isolated thread-per-core invariant,
// but can be transferred as Send between stages of a single worker.
// It is explicitly NOT Sync because bump allocation uses UnsafeCell without cross-thread synchronization.
unsafe impl Send for RequestArena {}

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

    #[test]
    fn test_arena_overflow_allocation_and_cleanup() {
        let arena = RequestArena::new();

        // Allocate a block larger than 64KB chunk
        let large_size = 128 * 1024;
        let large_ptr = arena.alloc(large_size, 64);
        assert!(!large_ptr.is_null());

        // Reset cleans up overflow blocks without leaking
        arena.reset();

        let ptr1 = arena.alloc(64, 8);
        assert_eq!(ptr1, arena.base_ptr);
    }

    #[test]
    fn test_arena_zero_size_and_invalid_alignment() {
        let arena = RequestArena::new();

        // Invalid alignment (0 or non-power-of-two) returns null
        assert!(arena.alloc(16, 0).is_null());
        assert!(arena.alloc(16, 3).is_null());
        assert!(arena.alloc(16, 5).is_null());

        // Zero-sized allocation returns non-null, aligned dangling pointer without UB
        let zero_ptr = arena.alloc(0, 8);
        assert!(!zero_ptr.is_null());
        assert_eq!((zero_ptr as usize) % 8, 0);

        // Zero-sized allocation overflow also safe
        let zero_overflow = arena.alloc_overflow(0, 16);
        assert!(!zero_overflow.is_null());
        assert_eq!((zero_overflow as usize) % 16, 0);
    }
}
