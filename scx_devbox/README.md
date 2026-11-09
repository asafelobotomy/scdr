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
| **Batch DSQ** | Served only when the interactive DSQ is empty; 12 ms slices for cache-friendly throughput. A per-CPU starvation guard has each CPU serve it at least every 60 ms. |
| **Two vtime clocks** | Interactive and batch tasks keep separate virtual clocks, so a nice-15 compiler racing ahead doesn't wipe out the interactive tasks' fairness history. |
| **Batch preemption** | An interactive wakeup that finds no idle CPU preempts a CPU running a batch task, once that task has had ≥1 ms (no thrashing). |
| **cgroup weights** | `cpu.weight` is honoured. Each cgroup keeps a flattened factor (product of weight/100 down from the root, fixed point); a task is charged vruntime and deadline at nice weight × cgroup factor, clamped to 1–10000, so weight 1 is deprioritised but never starved. |
| **Per-CPU kthreads** | Dispatched straight to their CPU. |
| **Idle CPU pick** | The kernel default: SMT-aware, prefers the previous CPU, keeps migrations low. |
| **CPU hotplug** | Handled, so suspend/resume and CPUs going offline don't unload it. If the kernel unloads it anyway, the loader restarts it in place when the kernel asks, or exits non-zero so systemd does. |

### How scdr drives it

scx_devbox reads the same per-task knobs scdr already sets, so scdr needs no
changes and your scdr rules keep meaning the same thing:

| scdr profile field | effect under scx_devbox |
|---|---|
| `sched = "batch"` / `"idle"` (`build`, `package-manager`, `indexer`) | always batch |
| `slice_us` (`audio`, `compositor`, `foreground`, `game`) | used as the task's slice; needs 4× the burst to be demoted to batch |
| `nice` < 0 (`desktop`, `compositor`, …) | needs 4× the burst to be demoted to batch |
| `nice` (any) | weights vruntime: nice 15 accrues it much faster |
| `cpu_weight` (`CPUWeight=` on units: compositor/audio 400, focused app 300, desktop 200, background 50, `background.slice` 30, baloo 1) | honoured: multiplies the task's weight, so a weight-400 unit accrues vruntime 4x slower than a default one |

Tasks no rule covers are classified by behaviour: an average run burst between
sleeps above 8 ms (with hysteresis) makes a task batch. Favoured tasks (nice < 0
or a slice hint) still are, at 32 ms: scdr's `foreground` role covers the whole
focused app, so when that app is Claude Desktop it also covers every node,
qemu or test runner the agent starts, and those must not compete as
interactive. Kernel threads are never demoted, which keeps reclaim (kswapd)
moving under memory pressure.

cgroup weights are an approximation of the kernel's hierarchical split. A
cgroup's factor is computed from its parent's when it is created; changing a
cgroup's weight later updates that cgroup only, not descendants that already
exist, and weights count against the default 100 rather than against siblings.
scdr sets weights on leaf-ish units, where this makes no difference.

scdr's own `[sched_ext]` switching goes through scx_loader, which only knows
the stock schedulers. Leave it off (the default) while scx_devbox runs; scdr's
rules, roles and pressure handling keep working underneath.

## Results

`tools/abtest.sh` runs `tools/latbench.c` under each scheduler in alternating
rounds, with 16 spinners: *sleep* is how late a thread sleeping 1 ms wakes,
*spawn* is fork + exec + reap of `/bin/true` back to back (an agent's tool
calls). i5-8300H (4c/8t), 16 GB, kernel 7.2.9, scdrd running, a desktop and
other builds in the background, 2 rounds of 6 s.

Spinners at nice 0 (`LAUNCH=service`):

| | sleep p50 | sleep p99 | spawn p99 | spawn max | spawns / 6 s | spinners |
|---|---|---|---|---|---|---|
| EEVDF | 56 µs | 3.1–3.5 ms | 6.6–8.4 ms | 32–49 ms | 2.4–2.6k | 100% |
| scx_devbox | 57 µs | 0.7–0.9 ms | 1.5–2.0 ms | 3.7–3.8 ms | 7.6–8.0k | ~95% |

Spinners inheriting scdr's foreground boost (`LAUNCH=scope`, the case of an
agent in the focused Claude Desktop starting CPU hogs):

| | sleep p50 | sleep p99 | spawn p99 | spawn max | spawns / 6 s |
|---|---|---|---|---|---|
| EEVDF | 56 µs | 2.1–2.6 ms | 6.7–7.7 ms | 17–30 ms | 2.7k |
| scx_devbox | 57 µs | 0.9–1.8 ms | 1.3–1.4 ms | 3.3–4.1 ms | 8.1–8.5k |

Overhead: about 0.6 s of BPF time per 10 s across 8 CPUs under that load
(`dispatch` ~1.6 µs, the rest under 1 µs per call); the loader uses ~2 MB.

## Build and run

Needs clang, bpftool, libbpf, and a kernel with sched_ext (6.12+).

```sh
make
sudo ./build/scx_devbox -i 2          # stats every 2 s; Ctrl-C to stop
tools/abtest.sh eevdf ./build/scx_devbox  # A/B against EEVDF (stop the service first)
```

Install as a service, either from scdr's Arch packaging (a split package:
`cd packaging/arch && makepkg -si` installs `scdr` and `scx-devbox`) or by
hand into `/usr/local`:

```sh
sudo make install
sudo systemctl enable --now scx_devbox
```

The build takes `vmlinux.h` from the running kernel's BTF, so it needs a
kernel with sched_ext; the result is CO-RE and loads on other sched_ext
kernels.

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
| `-H, --hint-thresh-mult` | 4 | threshold multiplier for nice < 0 / slice-hinted tasks |
| `-w, --batch-max-wait-ms` | 60 | per-CPU starvation guard |
| `-p, --batch-min-run-us` | 1000 | least run time before a batch task can be preempted |
| `-l, --lag-us` | 20000 | most sleep credit a waking task gets |
| `-n, --no-preempt` | | never preempt batch for interactive wakeups |
| `-i, --stats` | 0 | print counters every N seconds |

Stats: `direct_idle` (woke onto an idle CPU), `interactive` / `batch`
(enqueues per DSQ), `scdr_hint` (enqueues carrying an scdr slice),
`preempt_batch`, `idle_kick`, `starve_rescue`, `hint_demoted` (enqueues of a
favoured task demoted to batch as a hog), `cgroup_weighted` (enqueues of
tasks in a cgroup whose factor is not 1.0).
