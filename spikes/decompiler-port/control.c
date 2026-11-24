/* The control program: plain C, no libraries beyond the C runtime, so the port is
 * compared on the kind of input its own corpus holds. Imaginary `example` names only. */
#include <stdio.h>
#include <string.h>

struct example_record { int id; int qty; char label[16]; };

static int example_sum(const int *v, int n) {
    int s = 0;
    for (int i = 0; i < n; i++) s += v[i];
    return s;
}

static int example_find(const struct example_record *r, int n, int id) {
    for (int i = 0; i < n; i++)
        if (r[i].id == id) return i;
    return -1;
}

static unsigned example_hash(const char *s) {
    unsigned h = 5381;
    while (*s) h = h * 33 + (unsigned char)*s++;
    return h;
}

static long example_fact(int n) { return n <= 1 ? 1 : n * example_fact(n - 1); }

static int example_classify(int x) {
    switch (x) {
    case 0: return 10;
    case 1: return 20;
    case 2: return 35;
    case 3: return 41;
    case 7: return 99;
    default: return x < 0 ? -1 : 0;
    }
}

int main(int argc, char **argv) {
    struct example_record recs[3] = {{1, 5, "one"}, {2, 7, "two"}, {3, 9, "three"}};
    int qty[3] = {recs[0].qty, recs[1].qty, recs[2].qty};
    printf("%d %d %u %ld %d\n", example_sum(qty, 3), example_find(recs, 3, argc),
           example_hash(argc > 1 ? argv[1] : "example"), example_fact(argc + 5),
           example_classify(argc));
    return 0;
}
