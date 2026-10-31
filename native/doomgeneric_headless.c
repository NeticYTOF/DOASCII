#include "doomgeneric.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#ifdef _WIN32
#include <fcntl.h>
#endif

#ifdef _WIN32
#include <io.h>
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/select.h>
#include <time.h>
#include <unistd.h>
#endif

#define INPUT_QUEUE_SIZE 256

static uint16_t key_queue[INPUT_QUEUE_SIZE];
static unsigned int key_read_index;
static unsigned int key_write_index;

static int input_closed;

static int input_available(void)
{
#ifdef _WIN32
    DWORD available = 0;
    HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
    if (input == INVALID_HANDLE_VALUE || !PeekNamedPipe(input, NULL, 0, NULL, &available, NULL)) {
        input_closed = 1;
        return 0;
    }
    return available >= 2;
#else
    fd_set read_set;
    struct timeval timeout = {0, 0};
    FD_ZERO(&read_set);
    FD_SET(STDIN_FILENO, &read_set);
    return select(STDIN_FILENO + 1, &read_set, NULL, NULL, &timeout) > 0;
#endif
}

static void read_input_events(void)
{
    while (!input_closed && input_available()) {
        unsigned char event[2];
#ifdef _WIN32
        DWORD received = 0;
        if (!ReadFile(GetStdHandle(STD_INPUT_HANDLE), event, sizeof(event), &received, NULL) || received != sizeof(event)) {
            input_closed = 1;
            return;
        }
#else
        ssize_t received = read(STDIN_FILENO, event, sizeof(event));
        if (received != (ssize_t)sizeof(event)) {
            if (received == 0) input_closed = 1;
            return;
        }
#endif
        unsigned int next = (key_write_index + 1) % INPUT_QUEUE_SIZE;
        if (next != key_read_index) {
            key_queue[key_write_index] = ((uint16_t)event[0] << 8) | event[1];
            key_write_index = next;
        }
    }
}

void DG_Init(void)
{
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    memset(key_queue, 0, sizeof(key_queue));
}

void DG_DrawFrame(void)
{
    /* DGF1 + width:u16le + height:u16le + pixel_bytes:u8 + reserved[3]. */
    unsigned char header[12] = {
        'D', 'G', 'F', '1',
        (unsigned char)(DOOMGENERIC_RESX & 0xff),
        (unsigned char)((DOOMGENERIC_RESX >> 8) & 0xff),
        (unsigned char)(DOOMGENERIC_RESY & 0xff),
        (unsigned char)((DOOMGENERIC_RESY >> 8) & 0xff),
        4, 0, 0, 0
    };
    size_t pixels = (size_t)DOOMGENERIC_RESX * (size_t)DOOMGENERIC_RESY;
    if (fwrite(header, 1, sizeof(header), stdout) != sizeof(header) ||
        fwrite(DG_ScreenBuffer, sizeof(uint32_t), pixels, stdout) != pixels ||
        fflush(stdout) != 0) {
        input_closed = 1;
    }
}

void DG_SleepMs(uint32_t ms)
{
#ifdef _WIN32
    Sleep(ms);
#else
    struct timespec delay = {(time_t)(ms / 1000), (long)(ms % 1000) * 1000000L};
    nanosleep(&delay, NULL);
#endif
}

uint32_t DG_GetTicksMs(void)
{
#ifdef _WIN32
    return GetTickCount();
#else
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (uint32_t)(now.tv_sec * 1000 + now.tv_nsec / 1000000);
#endif
}

int DG_GetKey(int *pressed, unsigned char *key)
{
    read_input_events();
    if (key_read_index == key_write_index) return 0;
    uint16_t event = key_queue[key_read_index];
    key_read_index = (key_read_index + 1) % INPUT_QUEUE_SIZE;
    *pressed = event >> 8;
    *key = (unsigned char)(event & 0xff);
    return 1;
}

void DG_SetWindowTitle(const char *title)
{
    (void)title;
}
