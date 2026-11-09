# scx_devbox

A sched_ext scheduler for low-end machines (4c/8t laptops, tight RAM) used for
code development with LLM agent apps such as Claude Desktop and Claude Code.
It is designed to sit under [scdr](../README.md): scdr decides *what* each
process is, and scx_devbox turns that into scheduling.

Unlike the rest of scdr (MPL-2.0), this directory is GPL-2.0: the kernel only
loads sched_ext schedulers with a GPL-compatible license.

## The workload

- A compositor and a few Electron/Chromium processes that must stay smooth
  while you read and type.
- An agent firing off many short tool processes (git, rg, sh, node, test
  runners) and waiting on each one: their latency is the agent's latency.
- Occasional CPU hogs (rustc, cc1, ld, qemu, rust-analyzer) that will take
  every core you have.
- Not enough memory, so kswapd and kworkers need CPU at the worst moments.

## Policy

| | |
|---|---|
| **Interactive DSQ** | Ordered by deadline = vruntime + expected burst, so tasks that run briefly and sleep go first. Slice 3 ms, shrinking toward 0.5 ms as the queue grows. |
| **Batch DSQ** | Served only when the interactive DSQ is empty; 12 ms slices for cache-friendly throughput. A starvation guard serves it at least every 60 ms. |
| **Batch preemption** | An interactive wakeup that finds no idle CPU preempts a CPU running a batch task, once that task has had ≥1 ms (no thrashing). |
| **Per-CPU kthreads** | Dispatched straight to their CPU. |
| **Idle CPU pick** | The kernel default: SMT-aware, prefers the previous CPU, keeps migrations low. |

### How scdr drives it

scx_devbox reads the same per-task knobs scdr already sets, so scdr needs no
changes and your scdr rules keep meaning the same thing:

| scdr profile field | effect under scx_devbox |
|---|---|
| `sched = "batch"` / `"idle"` (`build`, `package-manager`, `indexer`) | always batch |
| `slice_us` (`audio`, `compositor`, `foreground`, `game`) | used as the task's slice; never demoted to batch |
| `nice` < 0 (`desktop`, `compositor`, …) | never demoted to batch |
| `nice` (any) | weights vruntime: nice 15 accrues it much faster |

Tasks no rule covers are classified by behaviour: an average run burst between
sleeps above 8 ms (with hysteresis) makes a task batch. Kernel threads are
never demoted, which keeps reclaim (kswapd) moving under memory pressure.

scdr's own `[sched_ext]` switching goes through scx_loader, which only knows
the stock schedulers. Leave it off (the default) while scx_devbox runs; scdr's
rules, roles and pressure handling keep working underneath.

## Results

`tools/latbench.c`: a thread sleeping 1 ms and doing a little work, while 16
spinners run. i5-8300H (4c/8t), 16 GB, kernel 7.2.9, scdrd running, 3 rounds
of 8 s each:

| | p99 wake latency | p99.9 | max | spinner throughput |
|---|---|---|---|---|
| EEVDF | 2.6–2.9 ms | 4.1–5.1 ms | 5.6–6.5 ms | ~1930 M/s |
| scx_devbox | 0.7–0.9 ms | 1.8–2.3 ms | 4.1–5.4 ms | ~1845 M/s (−4.5%) |

It also ran under a live 8-way `cargo build` with no watchdog stalls.

## Build and run

Needs clang, bpftool, libbpf, and a kernel with sched_ext (6.12+).

```sh
make
sudo ./build/scx_devbox -i 2          # stats every 2 s; Ctrl-C to stop
cc -O2 tools/latbench.c -o build/latbench && ./build/latbench 8
```

Install as a service:

```sh
sudo make install
sudo systemctl enable --now scx_devbox
```

Stop any scx_loader-started scheduler first (`scxctl stop`): only one sched_ext
scheduler can run. If the kernel ejects scx_devbox (a stall or an error), the
system is already back on EEVDF; the unit retries 3 times and then stays there.

## Tuning

| option | default | |
|---|---|---|
| `-s, --slice-us` | 3000 | interactive slice |
| `-m, --slice-min-us` | 500 | slice floor under load |
| `-b, --batch-slice-us` | 12000 | batch slice |
| `-t, --batch-thresh-us` | 8000 | average burst that makes a task batch |
| `-w, --batch-max-wait-ms` | 60 | starvation guard |
| `-p, --batch-min-run-us` | 1000 | least run time before a batch task can be preempted |
| `-l, --lag-us` | 20000 | most sleep credit a waking task gets |
| `-n, --no-preempt` | | never preempt batch for interactive wakeups |
| `-i, --stats` | 0 | print counters every N seconds |

Stats: `direct_idle` (woke onto an idle CPU), `interactive` / `batch`
(enqueues per DSQ), `scdr_hint` (enqueues carrying an scdr slice),
`preempt_batch`, `idle_kick`, `starve_rescue`.
