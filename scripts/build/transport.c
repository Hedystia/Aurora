/* Native script endpoint for Aurora protocol 2. */
#include <errno.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

enum { HEADER = 16, PAYLOAD = 32, CAPACITY = 8, FRAME = HEADER + PAYLOAD };

struct ring {
    _Atomic uint32_t write_cursor;
    _Atomic uint32_t read_cursor;
    uint8_t slots[CAPACITY][PAYLOAD];
};

_Static_assert(sizeof(struct ring) == 264, "Aurora ring layout");
_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "Aurora cursors must be lock-free");
static struct ring *rings;

static int transfer(int fd, uint8_t *data, size_t size, int writing) {
    size_t done = 0;
    while (done < size) {
        ssize_t count = writing ? write(fd, data + done, size - done)
                                : read(fd, data + done, size - done);
        if (count < 0 && errno == EINTR) continue;
        if (count == 0 && !writing && done == 0) return 0;
        if (count <= 0) return -1;
        done += (size_t)count;
    }
    return 1;
}

int32_t aurora_receive(uint8_t *bytes, size_t size) {
    if (size != FRAME) return -1;
    if (!rings) {
        void *mapping = mmap(NULL, 2 * sizeof(*rings), PROT_READ | PROT_WRITE, MAP_SHARED, 3, 0);
        if (mapping == MAP_FAILED) return -1;
        rings = mapping;
        close(3);
    }
    int received = transfer(0, bytes, HEADER, 0);
    if (received <= 0) return received;
    struct ring *ring = &rings[0];
    uint32_t read = atomic_load_explicit(&ring->read_cursor, memory_order_relaxed);
    uint32_t write = atomic_load_explicit(&ring->write_cursor, memory_order_acquire);
    if (write == read || write - read > CAPACITY) return -1;
    memcpy(bytes + HEADER, ring->slots[read % CAPACITY], PAYLOAD);
    atomic_store_explicit(&ring->read_cursor, read + 1, memory_order_release);
    return 1;
}

int32_t aurora_reply(uint8_t *bytes, size_t size) {
    if (size != FRAME || !rings) return -1;
    struct ring *ring = &rings[1];
    uint32_t write = atomic_load_explicit(&ring->write_cursor, memory_order_relaxed);
    uint32_t read = atomic_load_explicit(&ring->read_cursor, memory_order_acquire);
    if (write - read >= CAPACITY) return -1;
    memcpy(ring->slots[write % CAPACITY], bytes + HEADER, PAYLOAD);
    atomic_store_explicit(&ring->write_cursor, write + 1, memory_order_release);
    return transfer(0, bytes, HEADER, 1);
}
