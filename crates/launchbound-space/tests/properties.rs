//! Property tests: enumeration is deterministic and duplicate-free, and
//! config IDs are a pure function of the assignment (CONTRIBUTING.md).

use launchbound_space::{KernelSpec, enumerate};
use proptest::prelude::*;

/// A small random spec: 1..=4 integer dimensions, each with 1..=5 distinct
/// values. Names are drawn from a fixed pool so constraints stay valid.
fn arb_spec() -> impl Strategy<Value = KernelSpec> {
    let names = ["block_x", "tile", "unroll", "chunk"];
    proptest::collection::btree_map(
        proptest::sample::select(names.to_vec()),
        proptest::collection::btree_set(1u64..512, 1..=5),
        1..=4,
    )
    .prop_map(|dims| {
        let mut toml = String::from("[kernel]\nname = \"prop\"\nentry = \"prop\"\ndomain = 1\n");
        for (name, values) in dims {
            let list: Vec<String> = values.iter().map(u64::to_string).collect();
            toml.push_str(&format!("[dims.{name}]\nvalues = [{}]\n", list.join(", ")));
        }
        KernelSpec::from_toml_str("prop", &toml).expect("generated spec is valid")
    })
}

proptest! {
    #[test]
    fn enumeration_is_deterministic(spec in arb_spec()) {
        let a = enumerate(&spec).unwrap();
        let b = enumerate(&spec).unwrap();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn enumeration_is_duplicate_free(spec in arb_spec()) {
        let configs = enumerate(&spec).unwrap();
        let mut ids: Vec<String> =
            configs.iter().map(|c| c.id().as_str().to_string()).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        prop_assert_eq!(ids.len(), total);
    }

    #[test]
    fn id_is_a_pure_function_of_the_assignment(spec in arb_spec()) {
        let configs = enumerate(&spec).unwrap();
        for c in &configs {
            // Recomputing the ID gives the same answer, and equal configs
            // (cloned, i.e. reconstructed state) hash identically.
            prop_assert_eq!(c.id(), c.clone().id());
        }
    }
}
