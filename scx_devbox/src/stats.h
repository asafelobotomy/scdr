/* SPDX-License-Identifier: GPL-2.0 */
/*
 * Scheduler statistics for the loader: the counters in the BPF `stats`
 * map, the mean frequency target, and how long focused tasks wait to run.
 *
 * Every second a sample goes into a ring of the last STATS_RING, written
 * as JSON to /run/scx_devbox/stats (atomically, by rename). scdr reads it
 * on its tick and passes new samples on to its panel widget, which graphs
 * them; each sample's seq lets a reader take only the ones it hasn't seen.
 * With -i the loader also prints counters every N seconds.
 */
#ifndef __DEVBOX_STATS_H
#define __DEVBOX_STATS_H

#include <stdio.h>
#include <time.h>

#define STATS_FILE	"/run/scx_devbox/stats"
#define STATS_RING	10

static const char *stat_names[DEVBOX_NR_STATS] = {
	[DST_DIRECT_IDLE]   = "direct_idle",
	[DST_KTHREAD]       = "kthread",
	[DST_INTERACTIVE]   = "interactive",
	[DST_BATCH]         = "batch",
	[DST_HINTED]        = "scdr_hint",
	[DST_PREEMPT_KICK]  = "preempt_batch",
	[DST_IDLE_KICK]     = "idle_kick",
	[DST_STARVE_RESCUE] = "starve_rescue",
	[DST_HINT_DEMOTED]  = "hint_demoted",
	[DST_CGROUP_WEIGHTED] = "cgroup_weighted",
	[DST_PERF_SET]      = "perf_set",
	[DST_FOCUS]         = "focus",
	[DST_BG_DAMPED]     = "bg_damped",
};

struct sample {
	unsigned long long seq;
	long long time;			/* unix seconds */
	unsigned long long d[DEVBOX_NR_STATS];	/* per-second counts */
	unsigned int perf_pct;		/* mean frequency target, % */
	double wait_max_ms, wait_avg_ms;	/* focused tasks, queued to running */
};

static struct sample ring[STATS_RING];
static unsigned long long sample_seq;

static void read_stats(struct scx_devbox *skel, int ncpu,
		       unsigned long long out[DEVBOX_NR_STATS])
{
	unsigned long long vals[ncpu];
	int fd = bpf_map__fd(skel->maps.stats);

	for (__u32 i = 0; i < DEVBOX_NR_STATS; i++) {
		out[i] = 0;
		if (bpf_map_lookup_elem(fd, &i, vals))
			continue;
		for (int c = 0; c < ncpu; c++)
			out[i] += vals[c];
	}
}

/* Mean frequency target over CPUs that have one, of 1024. */
static unsigned int perf_avg(struct scx_devbox *skel, int ncpu)
{
	unsigned long long sum = 0, n = 0;

	for (int c = 0; c < ncpu && c < 512; c++) {
		if (skel->bss->cpu_perf[c].perf) {
			sum += skel->bss->cpu_perf[c].perf;
			n++;
		}
	}
	return n ? sum / n : 1024;
}

static void stats_write(void)
{
	char tmp[] = STATS_FILE ".tmp";
	FILE *f = fopen(tmp, "w");
	int first = 1;

	if (!f)
		return;
	fputs("{\"samples\":[", f);
	for (unsigned long long s = sample_seq > STATS_RING ? sample_seq - STATS_RING + 1 : 1;
	     s <= sample_seq; s++) {
		const struct sample *x = &ring[s % STATS_RING];

		fprintf(f, "%s{\"seq\":%llu,\"time\":%lld,\"interactive\":%llu,\"batch\":%llu,"
			"\"preempt\":%llu,\"damped\":%llu,\"focus\":%llu,\"rescue\":%llu,"
			"\"perf_pct\":%u,\"wait_max_ms\":%.2f,\"wait_avg_ms\":%.3f}",
			first ? "" : ",", x->seq, x->time, x->d[DST_INTERACTIVE], x->d[DST_BATCH],
			x->d[DST_PREEMPT_KICK], x->d[DST_BG_DAMPED], x->d[DST_FOCUS],
			x->d[DST_STARVE_RESCUE], x->perf_pct, x->wait_max_ms, x->wait_avg_ms);
		first = 0;
	}
	fputs("]}\n", f);
	if (fclose(f) == 0)
		rename(tmp, STATS_FILE);
}

/* Once a second: take a sample from the counters and publish the ring. */
static void stats_tick(struct scx_devbox *skel, int ncpu, unsigned long long prev[DEVBOX_NR_STATS])
{
	unsigned long long cur[DEVBOX_NR_STATS], cnt;
	struct sample *x = &ring[++sample_seq % STATS_RING];

	read_stats(skel, ncpu, cur);
	x->seq = sample_seq;
	x->time = time(NULL);
	for (int i = 0; i < DEVBOX_NR_STATS; i++) {
		x->d[i] = cur[i] - prev[i];
		prev[i] = cur[i];
	}
	x->perf_pct = perf_avg(skel, ncpu) * 100 / 1024;
	/* Read and reset; racing with the scheduler only blurs one sample. */
	cnt = skel->bss->focus_wait_cnt;
	x->wait_max_ms = skel->bss->focus_wait_max_ns / 1e6;
	x->wait_avg_ms = cnt ? skel->bss->focus_wait_sum_ns / 1e6 / cnt : 0;
	skel->bss->focus_wait_max_ns = 0;
	skel->bss->focus_wait_sum_ns = 0;
	skel->bss->focus_wait_cnt = 0;
	stats_write();
}

/* -i: the counters summed over the last @n samples, on one line. */
static void stats_print(int n)
{
	unsigned long long sum[DEVBOX_NR_STATS] = { 0 };
	double wait_max = 0;

	for (int k = 0; k < n && k < STATS_RING && (unsigned long long)k < sample_seq; k++) {
		const struct sample *x = &ring[(sample_seq - k) % STATS_RING];

		for (int i = 0; i < DEVBOX_NR_STATS; i++)
			sum[i] += x->d[i];
		if (x->wait_max_ms > wait_max)
			wait_max = x->wait_max_ms;
	}
	for (int i = 0; i < DEVBOX_NR_STATS; i++)
		printf("%s=%llu ", stat_names[i], sum[i]);
	printf("perf_avg=%u%% focus_wait_max=%.1fms\n", ring[sample_seq % STATS_RING].perf_pct, wait_max);
	fflush(stdout);
}

#endif
