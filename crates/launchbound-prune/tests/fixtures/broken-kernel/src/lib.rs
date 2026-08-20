//! Intentionally broken: `this_type_does_not_exist` forces rustc to fail
//! under the reconverge driver, which reports exit 2 (tool error). The gate
//! must surface that as a hard stop for every candidate.

mod params;

pub fn broken(_x: this_type_does_not_exist) {}
