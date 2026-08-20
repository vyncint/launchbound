//! Corpus kernel: tiled matmul C = A * B with rectangular tiles. See
//! README.md. A is MxK row-major, B is KxN row-major, C is MxN row-major;
//! M, N, K are exact multiples of TM, TN, TK.

mod params;

use cuda_device::{
    DisjointSlice, SharedArray, cuda_module, kernel, launch_bounds, launch_contract, thread,
};
use params::{LB_MAX, TK, TM, TN};

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    #[launch_bounds(LB_MAX)]
    #[launch_contract(domain = 2, coordinates = u32)]
    pub fn matmul(
        a: &[f32],
        b: &[f32],
        mut c: DisjointSlice<f32, thread::Runtime2DIndex>,
        k: u32,
        n: u32,
    ) {
        static mut TILE_A: SharedArray<f32, { TM * TK }> = SharedArray::UNINIT;
        static mut TILE_B: SharedArray<f32, { TK * TN }> = SharedArray::UNINIT;

        let tx = thread::threadIdx_x() as usize; // column within the tile
        let ty = thread::threadIdx_y() as usize; // row within the tile
        let row = thread::blockIdx_y() as usize * TM + ty;
        let col = thread::blockIdx_x() as usize * TN + tx;
        let k = k as usize;
        let n = n as usize;

        let mut acc = 0.0f32;
        let mut kt = 0usize;
        while kt < k / TK {
            // Cooperative staging: threads stride the tile so any block
            // shape equal to (TN, TM) covers both tiles.
            let mut j = tx;
            while j < TK {
                unsafe {
                    TILE_A[ty * TK + j] = a[row * k + kt * TK + j];
                }
                j += TN;
            }
            let mut i = ty;
            while i < TK {
                unsafe {
                    TILE_B[i * TN + tx] = b[(kt * TK + i) * n + col];
                }
                i += TM;
            }
            thread::sync_threads();

            let mut kk = 0usize;
            while kk < TK {
                unsafe {
                    acc += TILE_A[ty * TK + kk] * TILE_B[kk * TN + tx];
                }
                kk += 1;
            }
            thread::sync_threads();
            kt += 1;
        }

        // Write through the runtime 2D index; with block == (TN, TM) its
        // coordinates equal (row, col) above. The host binds c's row width
        // to this same n.
        if let Some(c_idx) = thread::index_2d_runtime(&c) {
            if let Some(o) = c.get_mut(c_idx) {
                *o = acc;
            }
        }
    }
}
