# Validation records and generated artifacts

Git stores validation scripts, test fixtures, benchmark profiles, numerical
policies and concise reports with tested versions, outcomes and limitations.
Raw output is generated data and stays in ignored local directories:

- `benchmarks/results/`
- `docs/validation/interop-results/`
- `docs/validation/wireguard-results/`

Reports describe historical runs; their raw files are not distributed in this
repository. Existing local archives, including failed and interrupted runs,
remain available on the machine where they were collected. A fresh checkout
must generate its own reference and outputs before comparing measurements.
Use fresh output paths and retain failures alongside successful runs.

For shared or release evidence, publish a separately retained artifact archive
with its checksum and link it from the report. Exclude credentials, private
keys and sensitive diagnostics before publishing. CI can retain generated
output as job artifacts; committing those files is unnecessary.

Versioned benchmark bounds live in `benchmarks/policies/`. Reference hashes and
the comparison outcome belong to local run artifacts. See
[performance](../performance.md) for reference binding and comparison commands.


The [configuration record](phase11-configuration.md) covers endpoint
settings, bounded proxy DNS, DNS/IP SAN identity, CLI/operator guides and the
focused paired reactor regression. It keeps its executable provenance separate
from the earlier full qualification.

The [final review corrections](final-review-p2.md) cover finite commitment
sweeps and bounded interrupted syscall retries, their regression and shutdown
checks, and the separately bound focused performance comparison.
