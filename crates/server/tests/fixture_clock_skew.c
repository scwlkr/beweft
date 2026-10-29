/* Process-local wall-clock shift for the isolated macOS worker regression.
 * Monotonic clocks and the PostgreSQL server are deliberately untouched. */
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

static long shift_seconds(void) {
    const char *value = getenv("BEWEFT_TEST_CLOCK_OFFSET_SECONDS");
    return value ? strtol(value, NULL, 10) : 0;
}

static int fixture_clock_gettime(clockid_t id, struct timespec *value) {
    uint64_t nanos = clock_gettime_nsec_np(id);
    value->tv_sec = (time_t)(nanos / 1000000000ULL);
    value->tv_nsec = (long)(nanos % 1000000000ULL);
    if (id == CLOCK_REALTIME) value->tv_sec += shift_seconds();
    return 0;
}

__attribute__((used)) static const struct { const void *replacement; const void *original; }
fixture_interposers[] __attribute__((section("__DATA,__interpose"))) = {
    {(const void *)fixture_clock_gettime, (const void *)clock_gettime},
};
