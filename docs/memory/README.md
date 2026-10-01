# Memory

How this app releases memory, the rules that keep it that way, and the
recorded failures that produced those rules.

| Document | Contents |
| --- | --- |
| [rules.md](rules.md) | Binding rules for new code. Read before writing anything that allocates surfaces, caches, timers, or workers. |
| [audit.md](audit.md) | Subsystem audit against the rules, with evidence pointers. |
| [fling-gate.md](fling-gate.md) | When a page may start a raster while scrolling, and why. |
| [split-memory-findings.md](split-memory-findings.md) | Experiments that did not hold: rebuilding the reader after a split, and a larger render budget. |

Supporting records one level up:

| Document | Contents |
| --- | --- |
| [../memory-baseline.md](../memory-baseline.md) | Instrumentation and the baseline numbers. |
| [../runtime-split.md](../runtime-split.md) | The shell/frame architecture that makes release possible. |
| [../route-split-retrospective.md](../route-split-retrospective.md) | The three frame-lifecycle designs and their measurements. |
| [../architecture.md](../architecture.md) | How the reader is built and what the tests enforce. |
| [../session-ownership.md](../session-ownership.md) | Ownership table for every engine resource. |
