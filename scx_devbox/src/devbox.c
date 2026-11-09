// SPDX-License-Identifier: GPL-2.0
/* Userspace loader for scx_devbox. */
#include <errno.h>
#include <getopt.h>
#include <signal.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <linux/types.h>
#include <bpf/bpf.h>

typedef __u8 u8;
typedef __u32 u32;
typedef __s32 s32;
typedef __u64 u64;
#include <bpf/libbpf.h>
#include "devbox.h"
#include "devbox.bpf.skel.h"

#define SCX_EXIT_ERROR 1024

static volatile sig_atomic_t stop;
static bool verbose;

static const char *stat_names[DEVBOX_NR_STATS] = {
	[DST_DIRECT_IDLE]   = "direct_idle",
	[DST_KTHREAD]       = "kthread",
	[DST_INTERACTIVE]   = "interactive",
	[DST_BATCH]         = "batch",
	[DST_HINTED]        = "scdr_hint",
	[DST_PREEMPT_KICK]  = "preempt_batch",
	[DST_IDLE_KICK]     = "idle_kick",
	[DST_STARVE_RESCUE] = "starve_rescue",
};

static void usage(const char *prog)
{
	fprintf(stderr,
"Usage: %s [options]\n"
"\n"
"sched_ext scheduler for low-end machines running LLM agent apps and builds.\n"
"Honours scdr's per-process nice, sched policy and slice_us as hints.\n"
"\n"
"  -s, --slice-us N          interactive slice (default 3000)\n"
"  -m, --slice-min-us N      interactive slice floor under load (default 500)\n"
"  -b, --batch-slice-us N    batch slice (default 12000)\n"
"  -t, --batch-thresh-us N   avg burst above which a task is batch (default 8000)\n"
"  -w, --batch-max-wait-ms N serve batch at least this often (default 60)\n"
"  -l, --lag-us N            max sleep credit (default 20000)\n"
"  -p, --batch-min-run-us N  batch runs this long before it can be preempted\n"
"                            (default 1000)\n"
"  -n, --no-preempt          don't preempt batch tasks for interactive wakeups\n"
"  -i, --stats SECS          print stats every SECS seconds (0 = off, default 0)\n"
"  -v, --verbose             libbpf debug output\n"
"  -h, --help\n", prog);
}

static int libbpf_print(enum libbpf_print_level level, const char *fmt,
			va_list args)
{
	if (level == LIBBPF_DEBUG && !verbose)
		return 0;
	return vfprintf(stderr, fmt, args);
}

static void on_signal(int sig) { (void)sig; stop = 1; }

static unsigned long long parse_ull(const char *s, const char *what)
{
	char *end;
	unsigned long long v;

	errno = 0;
	v = strtoull(s, &end, 10);
	if (errno || *end || !*s) {
		fprintf(stderr, "invalid %s: %s\n", what, s);
		exit(2);
	}
	return v;
}

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

static void check_sched_ext_state(void)
{
	char buf[32] = "";
	FILE *f = fopen("/sys/kernel/sched_ext/state", "r");

	if (!f) {
		fprintf(stderr, "this kernel has no sched_ext "
			"(/sys/kernel/sched_ext missing)\n");
		exit(1);
	}
	if (fgets(buf, sizeof(buf), f) && strncmp(buf, "disabled", 8)) {
		char ops[64] = "?";
		FILE *o = fopen("/sys/kernel/sched_ext/root/ops", "r");

		if (o) {
			if (fgets(ops, sizeof(ops), o))
				ops[strcspn(ops, "\n")] = 0;
			fclose(o);
		}
		fprintf(stderr, "another sched_ext scheduler is running (%s); "
			"stop it first (scxctl stop)\n", ops);
		fclose(f);
		exit(1);
	}
	fclose(f);
}

int main(int argc, char **argv)
{
	static const struct option opts[] = {
		{ "slice-us",          required_argument, 0, 's' },
		{ "slice-min-us",      required_argument, 0, 'm' },
		{ "batch-slice-us",    required_argument, 0, 'b' },
		{ "batch-thresh-us",   required_argument, 0, 't' },
		{ "batch-max-wait-ms", required_argument, 0, 'w' },
		{ "lag-us",            required_argument, 0, 'l' },
		{ "batch-min-run-us",  required_argument, 0, 'p' },
		{ "no-preempt",        no_argument,       0, 'n' },
		{ "stats",             required_argument, 0, 'i' },
		{ "verbose",           no_argument,       0, 'v' },
		{ "help",              no_argument,       0, 'h' },
		{ 0 }
	};
	unsigned long long prev[DEVBOX_NR_STATS] = { 0 }, cur[DEVBOX_NR_STATS];
	unsigned int stats_secs = 0, ticks = 0;
	struct scx_devbox *skel;
	struct bpf_link *link;
	int ncpu, opt, ret = 0;

	libbpf_set_print(libbpf_print);

	skel = scx_devbox__open();
	if (!skel) {
		fprintf(stderr, "failed to open BPF skeleton\n");
		return 1;
	}

	while ((opt = getopt_long(argc, argv, "s:m:b:t:w:l:p:ni:vh", opts,
				  NULL)) != -1) {
		switch (opt) {
		case 's': skel->rodata->slice_ns = parse_ull(optarg, "slice") * 1000; break;
		case 'm': skel->rodata->slice_min_ns = parse_ull(optarg, "slice-min") * 1000; break;
		case 'b': skel->rodata->batch_slice_ns = parse_ull(optarg, "batch-slice") * 1000; break;
		case 't': skel->rodata->batch_thresh_ns = parse_ull(optarg, "batch-thresh") * 1000; break;
		case 'w': skel->rodata->batch_max_wait_ns = parse_ull(optarg, "batch-max-wait") * 1000000; break;
		case 'l': skel->rodata->lag_ns = parse_ull(optarg, "lag") * 1000; break;
		case 'p': skel->rodata->batch_min_run_ns = parse_ull(optarg, "batch-min-run") * 1000; break;
		case 'n': skel->rodata->preempt_batch = false; break;
		case 'i': stats_secs = parse_ull(optarg, "stats"); break;
		case 'v': verbose = true; break;
		case 'h': usage(argv[0]); return 0;
		default:  usage(argv[0]); return 2;
		}
	}
	if (skel->rodata->slice_min_ns > skel->rodata->slice_ns ||
	    !skel->rodata->slice_ns || !skel->rodata->batch_thresh_ns) {
		fprintf(stderr, "slices must be non-zero and slice-min <= slice\n");
		return 2;
	}

	check_sched_ext_state();

	ncpu = libbpf_num_possible_cpus();
	if (ncpu <= 0) {
		fprintf(stderr, "can't count CPUs\n");
		return 1;
	}
	skel->rodata->nr_cpus = ncpu;

	if (scx_devbox__load(skel)) {
		fprintf(stderr, "failed to load BPF programs (run with -v)\n");
		scx_devbox__destroy(skel);
		return 1;
	}

	link = bpf_map__attach_struct_ops(skel->maps.devbox_ops);
	if (!link) {
		fprintf(stderr, "failed to attach scheduler: %s\n",
			strerror(errno));
		scx_devbox__destroy(skel);
		return 1;
	}

	signal(SIGINT, on_signal);
	signal(SIGTERM, on_signal);
	printf("scx_devbox running on %d CPUs (slice %llu us, batch %llu us, "
	       "batch threshold %llu us)\n", ncpu,
	       skel->rodata->slice_ns / 1000, skel->rodata->batch_slice_ns / 1000,
	       skel->rodata->batch_thresh_ns / 1000);
	fflush(stdout);

	while (!stop && !skel->bss->exit_kind) {
		sleep(1);
		if (!stats_secs || ++ticks % stats_secs)
			continue;
		read_stats(skel, ncpu, cur);
		for (int i = 0; i < DEVBOX_NR_STATS; i++) {
			printf("%s=%llu%s", stat_names[i], cur[i] - prev[i],
			       i == DEVBOX_NR_STATS - 1 ? "\n" : " ");
			prev[i] = cur[i];
		}
		fflush(stdout);
	}

	bpf_link__destroy(link);

	if (skel->bss->exit_kind) {
		fprintf(stderr, "scheduler exited (kind %d): %s%s%s\n",
			skel->bss->exit_kind, skel->bss->exit_reason,
			skel->bss->exit_msg[0] ? ": " : "",
			skel->bss->exit_msg);
		if (skel->bss->exit_kind >= SCX_EXIT_ERROR)
			ret = 1;
	}
	scx_devbox__destroy(skel);
	return ret;
}
