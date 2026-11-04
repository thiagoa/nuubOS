/*
 * wcbench: copy speed into Panfrost buffers on the device (perf-power.md
 * 5.2). Copies SIZE bytes from malloc'd memory into a write-combined BO
 * (Mesa's default mapping), a cached BO (PANFROST_BO_WB_MMAP) and malloc'd
 * memory, with glibc memcpy and two NEON loops, in chunks of CHUNK bytes
 * (a draw's vertex range or a texture row), and prints GB/s.
 * Build: aarch64-...-gcc -O2 -o wcbench wcbench.c
 * Run:   wcbench [SIZE_KB [CHUNK]]   (default 2048 KB, whole buffer)
 */
#include <drm/panfrost_drm.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

static double now(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec / 1e9;
}

/* 64 bytes a step with q-register pairs (len multiple of 64, 16-aligned). */
static void copy_ldp_stp(void *dst, const void *src, size_t len)
{
	__asm__ volatile(
		"1:	ldp q0, q1, [%1], #32\n"
		"	ldp q2, q3, [%1], #32\n"
		"	stp q0, q1, [%0], #32\n"
		"	stp q2, q3, [%0], #32\n"
		"	subs %2, %2, #64\n"
		"	b.gt 1b\n"
		: "+r"(dst), "+r"(src), "+r"(len) : : "v0", "v1", "v2", "v3", "memory", "cc");
}

/* The same with non-temporal stores. */
static void copy_stnp(void *dst, const void *src, size_t len)
{
	__asm__ volatile(
		"1:	ldp q0, q1, [%1], #32\n"
		"	ldp q2, q3, [%1], #32\n"
		"	stnp q0, q1, [%0]\n"
		"	stnp q2, q3, [%0, #32]\n"
		"	add %0, %0, #64\n"
		"	subs %2, %2, #64\n"
		"	b.gt 1b\n"
		: "+r"(dst), "+r"(src), "+r"(len) : : "v0", "v1", "v2", "v3", "memory", "cc");
}

static void copy_libc(void *dst, const void *src, size_t len)
{
	memcpy(dst, src, len);
}

static void *bo_map(int fd, size_t size, unsigned flags)
{
	struct drm_panfrost_create_bo c = { .size = size, .flags = flags };
	struct drm_panfrost_mmap_bo m = { 0 };
	void *p;

	if (ioctl(fd, DRM_IOCTL_PANFROST_CREATE_BO, &c) != 0)
		return NULL;
	m.handle = c.handle;
	if (ioctl(fd, DRM_IOCTL_PANFROST_MMAP_BO, &m) != 0)
		return NULL;
	p = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, m.offset);
	return p == MAP_FAILED ? NULL : p;
}

int main(int argc, char **argv)
{
	size_t size = (argc > 1 ? strtoul(argv[1], NULL, 0) : 2048) * 1024;
	size_t chunk = argc > 2 ? strtoul(argv[2], NULL, 0) : size;
	int fd = open("/dev/dri/renderD128", O_RDWR | O_CLOEXEC);
	struct { const char *name; void *p; } dst[3];
	struct { const char *name; void (*fn)(void *, const void *, size_t); } fns[] = {
		{ "memcpy", copy_libc }, { "ldp/stp", copy_ldp_stp }, { "stnp", copy_stnp },
	};
	char *src = aligned_alloc(64, size);

	if (fd < 0 || !src || chunk < 64 || chunk % 64) {
		fprintf(stderr, "wcbench: no render node, no memory or CHUNK not a multiple of 64\n");
		return 1;
	}
	memset(src, 0x5a, size);
	dst[0].name = "wc-bo"; dst[0].p = bo_map(fd, size, PANFROST_BO_NOEXEC);
	dst[1].name = "wb-bo"; dst[1].p = bo_map(fd, size, PANFROST_BO_NOEXEC | PANFROST_BO_WB_MMAP);
	dst[2].name = "malloc"; dst[2].p = aligned_alloc(64, size);
	printf("size %zu KB chunk %zu B\n", size / 1024, chunk);
	for (int d = 0; d < 3; d++) {
		if (!dst[d].p) {
			printf("%-7s unavailable\n", dst[d].name);
			continue;
		}
		printf("%-7s", dst[d].name);
		for (unsigned f = 0; f < sizeof(fns) / sizeof(fns[0]); f++) {
			double best = 1e9;

			for (int r = 0; r < 20; r++) {
				double t = now();

				for (size_t o = 0; o < size; o += chunk)
					fns[f].fn((char *)dst[d].p + o, src + o, chunk);
				t = now() - t;
				if (t < best)
					best = t;
			}
			printf("  %s %.2f GB/s", fns[f].name, size / best / 1e9);
		}
		printf("\n");
	}
	return 0;
}
