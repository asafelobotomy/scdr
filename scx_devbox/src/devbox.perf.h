/* SPDX-License-Identifier: GPL-2.0 */
/*
 * CPU frequency targets for devbox.bpf.c (cpufreq through schedutil).
 *
 * The kernel starts every CPU's sched_ext performance target at the
 * maximum (SCX_CPUPERF_ONE) and only lowers it when the scheduler asks. So
 * under the schedutil governor, a scheduler that never calls
 * scx_bpf_cpuperf_set() keeps every busy CPU at top speed: heat, thermal
 * throttling and battery drain on exactly the low-end laptops devbox is
 * for. Drivers that pick frequencies themselves (intel_pstate in active
 * mode, amd-pstate in active/guided mode) ignore the target.
 *
 * Each CPU's utilisation is measured from the time its tasks run, over
 * PERF_WINDOW_NS windows ending on the scheduler tick. It rises quickly and
 * decays slowly, gets 25% headroom, and is scaled to the CPU's own capacity,
 * because the target is on the system-wide scale (a little core asking for
 * 1024 would pin itself at top speed). A task with an scdr latency hint
 * (foreground, audio, compositor) gets at least perf_floor_hinted. A new
 * target is only sent when it moves by more than PERF_STEP.
 */
#ifndef __DEVBOX_PERF_H
#define __DEVBOX_PERF_H

#define PERF_WINDOW_NS	10000000ULL	/* 10 ms */
#define PERF_STEP	32		/* of SCX_CPUPERF_ONE: ~3% */

const volatile bool cpufreq_ctl = true;
const volatile u32 perf_floor_hinted = 768;	/* 75% for latency-hinted tasks */

struct cpu_perf cpu_perf[MAX_CPUS];

static __always_inline void perf_running(s32 cpu, u64 now)
{
	struct cpu_perf *c = &cpu_perf[CPU_IDX(cpu)];

	if (!cpufreq_ctl)
		return;
	c->busy_since = now;
	if (!c->win_start)
		c->win_start = now;
}

static __always_inline void perf_stopping(s32 cpu, u64 now)
{
	struct cpu_perf *c = &cpu_perf[CPU_IDX(cpu)];

	if (!cpufreq_ctl || !c->busy_since)
		return;
	c->busy += now - c->busy_since;
	c->busy_since = 0;
}

/* From the tick of the task running on @cpu: close the window if it's due. */
static __always_inline void perf_tick(s32 cpu, struct task_struct *p, u64 now)
{
	struct cpu_perf *c = &cpu_perf[CPU_IDX(cpu)];
	u64 elapsed, busy, now_util;
	u32 target;

	if (!cpufreq_ctl)
		return;
	elapsed = now - c->win_start;
	if (!c->win_start || elapsed < PERF_WINDOW_NS)
		return;

	busy = c->busy;
	if (c->busy_since) {
		busy += now - c->busy_since;
		c->busy_since = now;
	}
	now_util = min64(busy * SCX_CPUPERF_ONE / elapsed, SCX_CPUPERF_ONE);
	/* Rise fast (a build starting), decay slowly (a short pause). */
	if (now_util > c->util)
		c->util = (c->util + 3 * now_util) / 4;
	else
		c->util = (3 * c->util + now_util) / 4;
	c->busy = 0;
	c->win_start = now;

	target = min64((u64)c->util * 5 / 4, SCX_CPUPERF_ONE);
	if (has_latency_hint(p) && target < perf_floor_hinted)
		target = perf_floor_hinted;
	target = (u64)target * scx_bpf_cpuperf_cap(cpu) / SCX_CPUPERF_ONE;

	if (c->perf && target + PERF_STEP > c->perf && c->perf + PERF_STEP > target)
		return;
	c->perf = target ? target : 1;
	scx_bpf_cpuperf_set(cpu, c->perf);
	stat_inc(DST_PERF_SET);
}

#endif
