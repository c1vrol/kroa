#include <stdint.h>

int64_t kroa_add(int64_t a, int64_t b) {
    return a + b;
}

typedef struct {
    int64_t x;
    int64_t y;
} CPoint;

int64_t kroa_point_sum(CPoint p) {
    return p.x + p.y;
}

int64_t kroa_strlen_c(const char *s) {
    int64_t n = 0;
    if (!s) return -1;
    while (s[n] != '\0') n++;
    return n;
}
