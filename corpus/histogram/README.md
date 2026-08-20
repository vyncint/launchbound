# histogram

A privatized shared-memory histogram, included for **shared-memory capacity
pressure** (corpus/README.md): the `bins` dimension scales the shared buffer
from 1 KB to 48 KB (12288 x u32), the edge of the default budget — the
territory where `reconverge`'s RC004 and its `--cc`-dependent capacity
context matter, and where a verdict at one compute capability does not
transfer to another.

Barriers separate the zero-init, accumulate, and flush phases and are all
unconditional. Accumulation uses block-scope shared atomics; the flush adds
into device-scope atomics in global memory.
