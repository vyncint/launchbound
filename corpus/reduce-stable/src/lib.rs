//! Corpus kernel: the known-stable reduction. Identical shape to
//! reduce-flip, but every barrier is unconditional. See README.md.

mod params;

use cuda_device::{
    DisjointSlice, SharedArray, cuda_module, kernel, launch_bounds, launch_contract, thread,
};
use params::{LB_MAX, TILE};

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    #[launch_bounds(LB_MAX)]
    #[launch_contract(domain = 1, coordinates = u32)]
    pub fn reduce(data: &[f32], mut out: DisjointSlice<f32>) {
        static mut SMEM: SharedArray<f32, TILE> = SharedArray::UNINIT;

        let tid = thread::threadIdx_x() as usize;
        let gid = thread::index_1d().get();

        unsafe {
            SMEM[tid % TILE] = data[gid % data.len()];
        }
        // Every thread reaches both barriers: no divergence hazard at any
        // block size.
        thread::sync_threads();
        thread::sync_threads();

        if tid == 0 {
            let mut acc = 0.0f32;
            let mut i = 0usize;
            while i < TILE {
                unsafe {
                    acc += SMEM[i];
                }
                i += 1;
            }
            if let Some(o) = out.get_mut(thread::index_1d()) {
                *o = acc;
            }
        }
    }
}
