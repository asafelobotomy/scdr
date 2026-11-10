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
#include <time.h>
#include <unistd.h>
#include <linux/types.h>
#include <bpf/bpf.h>

typedef __u8 u8;
typedef __u32 u32;
typedef __s32 s32;
typedef __u64 u64;
typedef __s64 s64;
#include <bpf/libbpf.h>
#include "devbox.h"
#include "devbox.bpf.skel.h"
#include "hints.h"
#include "stats.h"

#define SCX_EXIT_UNREG 64
#define SCX_EXIT_SYSRQ 67
#define SCX_ECODE_ACT_RESTART (1ULL << 48)

static volatile sig_atomic_t stop;
static bool verbose;

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
"  -H, --hint-thresh-mult N  threshold multiplier for nice<0 / scdr-hinted tasks\n"
"                            (default 4)\n"
"  -w, --batch-max-wait-ms N serve batch at least this often (default 60)\n"
"  -l, --lag-us N            max sleep credit (default 20000)\n"
"  -p, --batch-min-run-us N  batch runs this long before it can be preempted\n"
"                            (default 1000)\n"
"  -n, --no-preempt          don't preempt batch tasks for interactive wakeups\n"
"  -F, --no-cpufreq          leave CPU frequency targets alone (the kernel keeps\n"
"                            them at maximum; only matters under schedutil)\n"
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

/* Reads /sys/kernel/sched_ext/<name> into buf; false if unreadable. */
static bool read_scx(const char *name, char *buf, size_t len)
{
	char path[64];
	FILE *f;
	bool ok;

	snprintf(path, sizeof(path), "/sys/kernel/sched_ext/%s", name);
	f = fopen(path, "r");
	if (!f)
		return false;
	ok = fgets(buf, len, f) != NULL;
	fclose(f);
	if (ok)
		buf[strcspn(buf, "\n")] = 0;
	return ok;
}

/*
 * Another scheduler may hold sched_ext for a moment: scx_loader starts its
 * default at boot and scdr stops it a few seconds later, and scdr puts it
 * back while it restarts. Wait for it to go rather than fail straight away.
 */
#define BUSY_WAIT_SECS 30

static void check_sched_ext_state(void)
{
	char state[32] = "", ops[64] = "?";

	if (!read_scx("state", state, sizeof(state))) {
		fprintf(stderr, "this kernel has no sched_ext "
			"(/sys/kernel/sched_ext missing)\n");
		exit(1);
	}
	for (int i = 0; strcmp(state, "disabled"); i++) {
		if (i == 0) {
			read_scx("root/ops", ops, sizeof(ops));
			fprintf(stderr, "another sched_ext scheduler is running (%s); "
				"waiting up to %ds for it to stop\n", ops, BUSY_WAIT_SECS);
		}
		if (i >= BUSY_WAIT_SECS || stop) {
			fprintf(stderr, "%s is still running; stop it first (scxctl stop)\n", ops);
			exit(1);
		}
		sleep(1);
		if (!read_scx("state", state, sizeof(state)))
			exit(1);
	}
}

enum run_result { RUN_STOPPED, RUN_FAILED, RUN_RESTART };

static bool wait_sched_ext_disabled(void)
{
	for (int i = 0; i < 50; i++) {
		char buf[32] = "";
		FILE *f = fopen("/sys/kernel/sched_ext/state", "r");

		if (f) {
			if (!fgets(buf, sizeof(buf), f))
				buf[0] = 0;
			fclose(f);
		}
		if (!strncmp(buf, "disabled", 8))
			return true;
		usleep(100000);
	}
	return false;
}

static long long read_hotplug_seq(void)
{
	long long seq = 0;
	FILE *f = fopen("/sys/kernel/sched_ext/hotplug_seq", "r");

	if (f) {
		if (fscanf(f, "%lld", &seq) != 1)
			seq = 0;
		fclose(f);
	}
	return seq;
}

/* Loads and attaches the scheduler, then waits until it stops. */
static enum run_result run(const struct scx_devbox__rodata *cfg, int ncpu,
			   unsigned int stats_secs)
{
	unsigned long long prev[DEVBOX_NR_STATS] = { 0 };
	unsigned int ticks = 0;
	struct scx_devbox *skel;
	struct timespec last, now;
	int hints_fd;
	struct bpf_link *link;
	enum run_result ret = RUN_STOPPED;
	s32 kind;

	skel = scx_devbox__open();
	if (!skel) {
		fprintf(stderr, "failed to open BPF skeleton\n");
		return RUN_FAILED;
	}
	*skel->rodata = *cfg;
	skel->rodata->nr_cpus = ncpu;
	/* Lets the kernel refuse us if a CPU came or went while loading. */
	skel->struct_ops.devbox_ops->hotplug_seq = read_hotplug_seq();

	if (scx_devbox__load(skel)) {
		fprintf(stderr, "failed to load BPF programs (run with -v)\n");
		scx_devbox__destroy(skel);
		return RUN_FAILED;
	}
	link = bpf_map__attach_struct_ops(skel->maps.devbox_ops);
	if (!link) {
		fprintf(stderr, "failed to attach scheduler: %s\n", strerror(errno));
		scx_devbox__destroy(skel);
		return errno == EBUSY ? RUN_RESTART : RUN_FAILED;
	}

	printf("scx_devbox running on %d CPUs (slice %llu us, batch %llu us, "
	       "batch threshold %llu us)\n", ncpu,
	       (unsigned long long)cfg->slice_ns / 1000,
	       (unsigned long long)cfg->batch_slice_ns / 1000,
	       (unsigned long long)cfg->batch_thresh_ns / 1000);
	fflush(stdout);

	hints_announce();
	hints_fd = hints_watch();
	hints_load(skel, verbose);
	read_stats(skel, ncpu, prev);	/* samples count from here, not from load */
	clock_gettime(CLOCK_MONOTONIC, &last);
	while (!stop && !skel->bss->exit_kind) {
		/* Wake for hint changes at once; count whole seconds for stats. */
		if (hints_wait(&hints_fd, 1000))
			hints_load(skel, verbose);
		clock_gettime(CLOCK_MONOTONIC, &now);
		if (now.tv_sec == last.tv_sec)
			continue;
		last = now;
		/* scdr (re)started: watch its runtime directory again. */
		if (hints_fd < 0 && (hints_fd = hints_watch()) >= 0)
			hints_load(skel, verbose);
		stats_tick(skel, ncpu, prev);
		if (stats_secs && ++ticks % stats_secs == 0)
			stats_print(stats_secs);
	}

	if (hints_fd >= 0)
		close(hints_fd);
	unlink(HINTS_API);
	unlink(STATS_FILE);
	bpf_link__destroy(link);

	kind = skel->bss->exit_kind;
	if (kind && !(stop && kind == SCX_EXIT_UNREG)) {
		fprintf(stderr, "scheduler exited (kind %d, code %#llx): %s%s%s\n",
			kind, (unsigned long long)skel->bss->exit_code,
			skel->bss->exit_reason, skel->bss->exit_msg[0] ? ": " : "",
			skel->bss->exit_msg);
		if ((unsigned long long)skel->bss->exit_code & SCX_ECODE_ACT_RESTART)
			ret = RUN_RESTART;
		else if (kind != SCX_EXIT_SYSRQ)
			/* Unloaded without being asked: let systemd restart us. */
			ret = RUN_FAILED;
	}
	scx_devbox__destroy(skel);
	return ret;
}

int main(int argc, char **argv)
{
	static const struct option opts[] = {
		{ "slice-us",          required_argument, 0, 's' },
		{ "slice-min-us",      required_argument, 0, 'm' },
		{ "batch-slice-us",    required_argument, 0, 'b' },
		{ "batch-thresh-us",   required_argument, 0, 't' },
		{ "hint-thresh-mult",  required_argument, 0, 'H' },
		{ "batch-max-wait-ms", required_argument, 0, 'w' },
		{ "lag-us",            required_argument, 0, 'l' },
		{ "batch-min-run-us",  required_argument, 0, 'p' },
		{ "no-preempt",        no_argument,       0, 'n' },
		{ "no-cpufreq",        no_argument,       0, 'F' },
		{ "stats",             required_argument, 0, 'i' },
		{ "verbose",           no_argument,       0, 'v' },
		{ "help",              no_argument,       0, 'h' },
		{ 0 }
	};
	struct scx_devbox__rodata cfg;
	struct scx_devbox *skel;
	unsigned int stats_secs = 0, restarts = 0;
	enum run_result res;
	int ncpu, opt;

	libbpf_set_print(libbpf_print);

	/* The BPF object's initial rodata holds the defaults. */
	skel = scx_devbox__open();
	if (!skel) {
		fprintf(stderr, "failed to open BPF skeleton\n");
		return 1;
	}
	cfg = *skel->rodata;
	scx_devbox__destroy(skel);

	while ((opt = getopt_long(argc, argv, "s:m:b:t:H:w:l:p:nFi:vh", opts,
				  NULL)) != -1) {
		switch (opt) {
		case 's': cfg.slice_ns = parse_ull(optarg, "slice") * 1000; break;
		case 'm': cfg.slice_min_ns = parse_ull(optarg, "slice-min") * 1000; break;
		case 'b': cfg.batch_slice_ns = parse_ull(optarg, "batch-slice") * 1000; break;
		case 't': cfg.batch_thresh_ns = parse_ull(optarg, "batch-thresh") * 1000; break;
		case 'H': cfg.hint_thresh_mult = parse_ull(optarg, "hint-thresh-mult"); break;
		case 'w': cfg.batch_max_wait_ns = parse_ull(optarg, "batch-max-wait") * 1000000; break;
		case 'l': cfg.lag_ns = parse_ull(optarg, "lag") * 1000; break;
		case 'p': cfg.batch_min_run_ns = parse_ull(optarg, "batch-min-run") * 1000; break;
		case 'n': cfg.preempt_batch = false; break;
		case 'F': cfg.cpufreq_ctl = false; break;
		case 'i': stats_secs = parse_ull(optarg, "stats"); break;
		case 'v': verbose = true; break;
		case 'h': usage(argv[0]); return 0;
		default:  usage(argv[0]); return 2;
		}
	}
	if (cfg.slice_min_ns > cfg.slice_ns || !cfg.slice_ns ||
	    !cfg.batch_thresh_ns || !cfg.hint_thresh_mult) {
		fprintf(stderr, "slices, threshold and multiplier must be non-zero "
			"and slice-min <= slice\n");
		return 2;
	}

	signal(SIGINT, on_signal);
	signal(SIGTERM, on_signal);
	check_sched_ext_state();

	ncpu = libbpf_num_possible_cpus();
	if (ncpu <= 0) {
		fprintf(stderr, "can't count CPUs\n");
		return 1;
	}


	/* The kernel asks for a restart e.g. when CPUs change mid-load. */
	for (;;) {
		struct timespec t0, t1;

		clock_gettime(CLOCK_MONOTONIC, &t0);
		res = run(&cfg, ncpu, stats_secs);
		if (res != RUN_RESTART || stop)
			break;
		/* A run that lasted a while was healthy: count restarts afresh. */
		clock_gettime(CLOCK_MONOTONIC, &t1);
		if (t1.tv_sec - t0.tv_sec > 60)
			restarts = 0;
		if (++restarts > 10) {
			fprintf(stderr, "giving up after %u restarts\n", restarts - 1);
			return 1;
		}
		fprintf(stderr, "restarting scheduler\n");
		if (!wait_sched_ext_disabled())
			return 1;
	}
	return res == RUN_FAILED ? 1 : 0;
}
