//! Corpus kernel: block-level inclusive prefix scan, barrier-dense. See
//! README.md.

mod params;

use cuda_device::{
    DisjointSlice, SharedArray, cuda_module, kernel, launch_bounds, launch_contract, thread,
};
use params::{BLOCK_MAX, LB_MAX};

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    #[launch_bounds(LB_MAX)]
    #[launch_contract(domain = 1, coordinates = u32)]
    pub fn scan(data: &[u32], mut out: DisjointSlice<u32>) {
        static mut SMEM: SharedArray<u32, BLOCK_MAX> = SharedArray::UNINIT;

        let tid = thread::threadIdx_x() as usize;
        let gid = thread::index_1d().get();

        unsafe {
            SMEM[tid % BLOCK_MAX] = data[gid % data.len()];
        }
        thread::sync_threads();

        // Hillis-Steele: the loop bound is a compile-time constant, so the
        // loop itself is uniform; the divergent guard covers only the read.
        let mut stride = 1usize;
        while stride < BLOCK_MAX {
            let addend = if tid >= stride {
                unsafe { SMEM[(tid - stride) % BLOCK_MAX] }
            } else {
                0
            };
            thread::sync_threads();
            unsafe {
                SMEM[tid % BLOCK_MAX] = SMEM[tid % BLOCK_MAX].wrapping_add(addend);
            }
            thread::sync_threads();
            stride *= 2;
        }

        if let Some(o) = out.get_mut(thread::index_1d()) {
            unsafe {
                *o = SMEM[tid % BLOCK_MAX];
            }
        }
    }
}
