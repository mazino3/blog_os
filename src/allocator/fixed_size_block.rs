use super::Locked;
use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512]; // must each be power of 2

fn list_index(layout: &Layout) -> Option<usize> {
    let block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= block_size)
}

pub struct FixedSizeBlockAllocator {
    list_heads: [ListNode; BLOCK_SIZES.len()],
    backup_allocator: linked_list_allocator::Heap,
}

impl FixedSizeBlockAllocator {
    /// Creates an empty FixedSizeBlockAllocator.
    pub const fn new() -> Self {
        FixedSizeBlockAllocator {
            list_heads: [ListNode::new(); BLOCK_SIZES.len()],
            backup_allocator: linked_list_allocator::Heap::empty(),
        }
    }

    /// Initialize the allocator with the given heap bounds.
    ///
    /// This function is unsafe because the caller must guarantee that the given
    /// heap bounds are valid and that the heap is unused. This method must be
    /// called only once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.backup_allocator.init(heap_start, heap_size);
    }

    fn backup_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.backup_allocator.allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }
}

struct ListNode {
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new() -> Self {
        Self {next : None, }
    }
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                match allocator.list_heads[index].next.take() {
                    Some(node) => {
                        allocator.list_heads[index].next = node.next.take();
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // no block exists in list => allocate new block
                        let block_size = BLOCK_SIZES[index];
                        // only works if all block sizes are a power of 2
                        let block_align = block_size;
                        let layout = Layout::from_size_align(block_size, block_align).unwrap();
                        allocator.backup_alloc(layout)
                    }
                }           
            }
            None => allocator.backup_alloc(layout),
        }        
    }

    #[allow(unused_unsafe)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        if let Some(index) = list_index(&layout) {
            let new_node_ptr = ptr as *mut ListNode;
            let new_node = ListNode {
                next: allocator.list_heads[index].next.take(),
            };
            new_node_ptr.write(new_node);
            allocator.list_heads[index].next = Some(unsafe { &mut *new_node_ptr});
        } else {
            unsafe {
                allocator.backup_allocator.deallocate(NonNull::new(ptr).unwrap(), layout);
            }
        }
    }
}