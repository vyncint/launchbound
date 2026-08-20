//! Plan rendering against the real corpus specs (file-only; no compiler,
//! no GPU).

use launchbound_bench::{ArgSpec, BenchSpec};
use launchbound_space::{KernelSpec, enumerate};
use std::path::PathBuf;

fn corpus(kernel: &str) -> KernelSpec {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(kernel);
    KernelSpec::load(&dir).unwrap()
}

#[test]
fn reduce_flip_candidates_render_with_expressions_evaluated() {
    let spec = corpus("reduce-flip");
    let bench = BenchSpec::load(&spec).unwrap();
    let configs = enumerate(&spec).unwrap();
    let c32 = configs
        .iter()
        .find(|c| c.block_threads() == 32)
        .expect("block=32 exists");

    let candidate = bench.candidate(&spec, c32, "x.ptx").unwrap();
    assert_eq!(candidate.block, [32, 1, 1]);
    assert_eq!(candidate.grid, [1_048_576 / 32, 1, 1]);
    assert_eq!(
        candidate.args,
        vec![
            ArgSpec::InF32 { len: 1_048_576 },
            ArgSpec::LenOf { of: 0 },
            ArgSpec::OutF32 { len: 1_048_576 },
            ArgSpec::LenOf { of: 2 },
        ]
    );
    assert_eq!(candidate.id, c32.id().as_str());
}

#[test]
fn histogram_bins_expression_tracks_the_spec_dimension() {
    let spec = corpus("histogram");
    let bench = BenchSpec::load(&spec).unwrap();
    let configs = enumerate(&spec).unwrap();
    for config in &configs {
        let candidate = bench.candidate(&spec, config, "x.ptx").unwrap();
        let bins = match config.get("bins") {
            Some(launchbound_space::Value::Int(n)) => *n,
            _ => panic!("bins dim"),
        };
        assert_eq!(candidate.args[2], ArgSpec::OutU32 { len: bins });
        match candidate.args[0] {
            ArgSpec::InU32 { modulo, .. } => assert_eq!(modulo, bins),
            ref other => panic!("expected in_u32, got {other:?}"),
        }
    }
}
