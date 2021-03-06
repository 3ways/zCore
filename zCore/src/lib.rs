#![feature(lang_items)]
#![feature(naked_functions)]
#![feature(untagged_unions)]
#![feature(asm)]
#![feature(optin_builtin_traits)]
#![feature(panic_info_message)]
#![feature(global_asm)]
#![feature(alloc_prelude)]
#![deny(unused_must_use, unused_imports, warnings)]
#![deny(stable_features)]
#![deny(ellipsis_inclusive_range_patterns)]
#![no_std]

extern crate alloc;
#[macro_use]
extern crate log;
extern crate rlibc;

#[macro_use]
pub mod logging;
mod interrupt;
pub mod lang;
mod memory;
mod process;

use {
    buddy_system_allocator::LockedHeapWithRescue, kernel_hal_bare::arch::timer_init,
    rboot::BootInfo,
};

pub use memory::{hal_frame_alloc, hal_frame_dealloc, hal_pt_map_kernel};

#[no_mangle]
pub extern "C" fn _start(boot_info: &BootInfo) -> ! {
    logging::init();
    memory::init_heap();
    memory::init_frame_allocator(boot_info);
    info!("{:#x?}", boot_info);
    interrupt::init();
    timer_init();
    process::init();
    unreachable!();
}

/// Global heap allocator
///
/// Available after `memory::init_heap()`.
///
/// It should be defined in memory mod, but in Rust `global_allocator` must be in root mod.
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeapWithRescue = LockedHeapWithRescue::new(memory::enlarge_heap);
