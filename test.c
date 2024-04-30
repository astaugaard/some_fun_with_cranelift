
#include <stdio.h>

extern int increment_runtime  (int);

int increment_number_c(int i) {
    printf("%d",i);
    return (increment_runtime(i));
}

