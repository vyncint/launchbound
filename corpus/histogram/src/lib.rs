//! Corpus kernel: privatized shared-memory histogram. See README.md.

mod params;

use cuda_device::atomic::{AtomicOrdering, DeviceAtomicU32};
use cuda_device::{
    SharedArray, cuda_module, kernel, launch_bounds, launch_contract, thread,
};
use params::{BINS, LB_MAX};

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    #[launch_bounds(LB_MAX)]
    #[launch_contract(domain = 1, coordinates = u32)]
    pub fn histogram(data: &[u32], out: &[DeviceAtomicU32]) {
        static mut SMEM: SharedArray<u32, BINS> = SharedArray::UNINIT;

        let tid = thread::threadIdx_x() as usize;
        let width = thread::blockDim_x() as usize;
        let gid = thread::index_1d().get();

        // Zero the private histogram, strided across the block.
        let mut b = tid;
        while b < BINS {
            unsafe {
                SMEM[b] = 0;
            }
            b += width;
        }
        thread::sync_threads();

        // Accumulate this thread's element into shared bins.
        if gid < data.len() {
            let bin = (data[gid] as usize) % BINS;
            unsafe {
                let slot = DeviceAtomicU32::from_ptr(&raw mut SMEM[bin]);
                slot.fetch_add(1, AtomicOrdering::Relaxed);
            }
        }
        thread::sync_threads();

        // Flush the private histogram into the global one.
        let mut b = tid;
        while b < BINS {
            let count = unsafe { SMEM[b] };
            if count != 0 && b < out.len() {
                out[b].fetch_add(count, AtomicOrdering::Relaxed);
            }
            b += width;
        }
    }
}
