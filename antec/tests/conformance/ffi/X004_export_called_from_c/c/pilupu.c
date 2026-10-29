#include <stdint.h>

int32_t ante_kalupu(int32_t a, int32_t b);
void palakarinchu(void);

int32_t padhi_saarlu(int32_t a, int32_t b) { return ante_kalupu(a, b) * 10; }
void pilupu(void) { palakarinchu(); }
