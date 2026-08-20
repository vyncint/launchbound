//! Corpus kernel: 1D box stencil, no barriers. See README.md.

mod params;

use cuda_device::{
    DisjointSlice, cuda_module, kernel, launch_bounds, launch_contract, thread,
};
use params::{LB_MAX, RADIUS, UNROLL};

#[cuda_module]
mod kernels {
    use super::*;

    #[kernel]
    #[launch_bounds(LB_MAX)]
    #[launch_contract(domain = 1, coordinates = u32)]
    pub fn stencil(data: &[f32], mut out: DisjointSlice<f32>) {
        let gid = thread::index_1d().get();

        // Interior threads sum the full window; boundary threads copy.
        let value = if gid >= RADIUS && gid + RADIUS < data.len() {
            let mut acc = 0.0f32;
            let mut d = 0usize;
            #[unroll(UNROLL)]
            while d < 2 * RADIUS + 1 {
                acc += data[gid - RADIUS + d];
                d += 1;
            }
            acc
        } else if gid < data.len() {
            data[gid]
        } else {
            0.0
        };

        if let Some(o) = out.get_mut(thread::index_1d()) {
            *o = value;
        }
    }
}
