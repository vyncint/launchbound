// MSL counterpart of the reduce-stable fixture, for the Metal backend.
//
// NO CONVERGENCE GATE EXISTS ON THIS PATH: reconverge analyzes cuda-oxide
// kernels; there is no equivalent for MSL and this project does not build
// one. Apple GPUs have 32-wide SIMD-groups and simd-scoped collectives, so
// the same bug class exists here and is simply NOT checked (README, §3.4).
//
// The `constant constexpr` parameters below are rewritten per candidate by
// the tuner in a scratch copy, exactly like src/params.rs on the CUDA path.
#include <metal_stdlib>
using namespace metal;

constant constexpr uint TILE = 128;

kernel void reduce(
    device const float *data [[buffer(0)]],
    constant uint &data_len [[buffer(1)]],
    device float *out [[buffer(2)]],
    constant uint &out_len [[buffer(3)]],
    uint tid [[thread_position_in_threadgroup]],
    uint gid [[thread_position_in_grid]])
{
    threadgroup float smem[TILE];
    smem[tid % TILE] = data[gid % data_len];
    threadgroup_barrier(mem_flags::mem_threadgroup);
    threadgroup_barrier(mem_flags::mem_threadgroup);

    if (tid == 0) {
        float acc = 0.0f;
        for (uint i = 0; i < TILE; i++) {
            acc += smem[i];
        }
        if (gid < out_len) {
            out[gid] = acc;
        }
    }
}
