//! Nothing allocates after `Voice::new`: a counting global allocator watches this thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use warble_dsp::{Preset, Voice};

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

#[test]
fn process_set_preset_and_set_enabled_never_allocate() {
    let mut voice = Voice::new(48_000.0);
    let mut buf = vec![0.0f32; 4096];
    let mut seed = 1u32;
    let before = allocations();
    for round in 0..6 {
        for preset in Preset::ALL {
            voice.set_preset(preset);
            for len in [0, 1, 64, 480, 4096, 4096, 4096] {
                for x in &mut buf[..len] {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    *x = seed as f32 / u32::MAX as f32 - 0.5;
                }
                voice.process(&mut buf[..len]);
            }
            voice.set_enabled(round % 2 == 1);
            voice.process(&mut buf[..480]);
            voice.set_enabled(true);
        }
    }
    let after = allocations();
    assert_eq!(
        after - before,
        0,
        "audio-thread calls allocated {} times",
        after - before
    );
}
