#include <stdint.h>

typedef struct { int32_t x, y; } Point;
typedef struct { double a; float b; int8_t c; uint8_t tag[3]; } Mixed;

Point scale(Point p, int32_t k);

Point move_point(Point p, int32_t dx) { p.x += dx; return p; }
Mixed mix(Mixed m) { m.a *= 2; m.b += 1; m.c -= 1; m.tag[2] = 'z'; return m; }
Point twice(Point p) { return scale(p, 2); }
int16_t neg16(int16_t v) { return (int16_t)-v; }
uint8_t inc8(uint8_t v) { return (uint8_t)(v + 1); }
float half(float f) { return f / 2; }
