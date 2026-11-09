// SPDX-License-Identifier: GPL-2.0
/*
 * latbench: wake-up latency of an interactive-style task (sleeps 1 ms, runs
 * briefly) while 2x nCPU processes spin, plus the spinners' throughput.
 * Compare schedulers by running it under each.
 *
 *   latbench [seconds=10] [hogs=2*nproc] [sleep|spawn]
 *
 * "spawn" measures what an agent's tool calls see instead: the time to
 * fork, exec /bin/true and reap it, back to back.
 *
 * nice and scdr's slice hints are inherited, so start it as a transient
 * service to get a clean nice-0 baseline rather than the caller's profile:
 *   systemd-run --user --pipe --wait -q $PWD/latbench 8
 */
#define _GNU_SOURCE
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static long long now_ns(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static int cmp_ll(const void *a, const void *b)
{
	long long x = *(const long long *)a, y = *(const long long *)b;

	return (x > y) - (x < y);
}

int main(int argc, char **argv)
{
	int secs = argc > 1 ? atoi(argv[1]) : 10;
	int hogs = argc > 2 ? atoi(argv[2]) : 2 * sysconf(_SC_NPROCESSORS_ONLN);
	int spawn = argc > 3 && !strcmp(argv[3], "spawn");
	volatile unsigned long long *work;
	long long *lat, end;
	size_t n = 0, cap = (size_t)secs * 2000;
	pid_t pids[1024];
	unsigned long long total = 0;

	if (hogs > 1024)
		hogs = 1024;
	work = mmap(NULL, sizeof(*work) * hogs, PROT_READ | PROT_WRITE,
		    MAP_SHARED | MAP_ANONYMOUS, -1, 0);
	lat = malloc(cap * sizeof(*lat));
	if (work == MAP_FAILED || !lat)
		return 1;

	for (int i = 0; i < hogs; i++) {
		pids[i] = fork();
		if (!pids[i]) {
			for (;;)
				work[i]++;
		}
	}

	usleep(500000); /* let the scheduler classify the hogs */
	end = now_ns() + secs * 1000000000LL;
	while (now_ns() < end && n < cap) {
		struct timespec req = { 0, 1000000 };
		long long t0 = now_ns();

		if (spawn) {
			pid_t c = fork();

			if (!c) {
				execl("/bin/true", "true", (char *)NULL);
				_exit(127);
			}
			waitpid(c, NULL, 0);
			lat[n++] = now_ns() - t0;
			continue;
		}
		nanosleep(&req, NULL);
		lat[n++] = now_ns() - t0 - 1000000;
		for (volatile int k = 0; k < 20000; k++)
			; /* a little work, like handling an event */
	}

	for (int i = 0; i < hogs; i++)
		kill(pids[i], SIGKILL);
	for (int i = 0; i < hogs; i++)
		waitpid(pids[i], NULL, 0);
	for (int i = 0; i < hogs; i++)
		total += work[i];

	qsort(lat, n, sizeof(*lat), cmp_ll);
	printf("%s samples=%zu p50=%lldus p99=%lldus p99.9=%lldus max=%lldus "
	       "hog_throughput=%.1fM/s\n", spawn ? "spawn" : "sleep", n,
	       lat[n / 2] / 1000,
	       lat[n * 99 / 100] / 1000, lat[n * 999 / 1000] / 1000,
	       lat[n - 1] / 1000, total / 1e6 / (secs + 0.5));
	return 0;
}
