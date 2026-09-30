# Memory

How this app releases memory, the rules that keep it that way, and the
recorded failures that produced those rules.

| Document | Contents |
| --- | --- |
| [rules.md](rules.md) | Binding rules for new code. Read before writing anything that allocates surfaces, caches, timers, or workers. |
| [audit.md](audit.md) | Subsystem audit against the rules, with evidence pointers. |
| [fling-gate.md](fling-gate.md) | The scroll-churn regression and the dwell fix, with before/after code. |

Supporting records one level up:

| Document | Contents |
| --- | --- |
| [../memory-baseline.md](../memory-baseline.md) | Instrumentation and the Phase 0 baseline numbers. |
| [../runtime-split.md](../runtime-split.md) | The shell/frame architecture that makes release possible. |
| [../route-split-retrospective.md](../route-split-retrospective.md) | The three frame-lifecycle designs and their measurements. |
| [../session-ownership.md](../session-ownership.md) | Ownership table for every engine resource. |
