//! Corpus kernel: shared-memory reduction with a warp-leader barrier — the
//! known-flip case. See README.md; keep this minimal and readable, it is a
//! fixture, not a product.

mod params;

use cuda_device::{
    DisjointSlice, SharedArray, cuda_module, kernel, launch_bounds, launch_contract, thread, warp,
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
        thread::sync_threads();

        // The flip: uniform within one warp, divergent across a block.
        // Safe iff the block is a single warp (block_x <= 32).
        if warp::warp_id() == 0 {
            thread::sync_threads();
        }

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
