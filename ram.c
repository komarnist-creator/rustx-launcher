/* rustx: маленький нативный модуль на C - читает объём оперативной памяти.
   Вызывается из Rust (sys.rs). Значения в мегабайтах. Возвращает 1 при успехе. */
#include <stdint.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

int rustx_ram_mb(uint64_t *total, uint64_t *avail) {
    MEMORYSTATUSEX m;
    m.dwLength = sizeof(m);
    if (!GlobalMemoryStatusEx(&m)) {
        return 0;
    }
    *total = (uint64_t)(m.ullTotalPhys / 1048576ULL);
    *avail = (uint64_t)(m.ullAvailPhys / 1048576ULL);
    return 1;
}
#else
int rustx_ram_mb(uint64_t *total, uint64_t *avail) {
    (void)total;
    (void)avail;
    return 0;
}
#endif
