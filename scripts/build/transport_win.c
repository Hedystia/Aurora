/* Native script endpoint for Aurora protocol 19 on a Windows named pipe. */
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <windows.h>

enum { FRAME = 60 };

static HANDLE pipe = INVALID_HANDLE_VALUE;

static int32_t open_pipe(void) {
    if (pipe != INVALID_HANDLE_VALUE) return 1;
    const char *name = getenv("AURORA_SCRIPT_PIPE");
    if (name == NULL || *name == '\0') return -1;
    int count = MultiByteToWideChar(CP_UTF8, 0, name, -1, NULL, 0);
    if (count <= 0) return -1;
    wchar_t *wide = (wchar_t *)malloc((size_t)count * sizeof(wchar_t));
    if (wide == NULL) return -1;
    if (MultiByteToWideChar(CP_UTF8, 0, name, -1, wide, count) <= 0) {
        free(wide);
        return -1;
    }
    /* The player creates the server and passes its unique name to this child. */
    pipe = CreateFileW(wide, GENERIC_READ | GENERIC_WRITE, 0, NULL, OPEN_EXISTING,
                       FILE_ATTRIBUTE_NORMAL, NULL);
    free(wide);
    if (pipe == INVALID_HANDLE_VALUE) return -1;
    DWORD mode = PIPE_READMODE_BYTE;
    if (!SetNamedPipeHandleState(pipe, &mode, NULL, NULL)) return -1;
    return 1;
}

static int32_t transfer(uint8_t *bytes, size_t size, int writing) {
    size_t done = 0;
    while (done < size) {
        DWORD count = 0;
        BOOL ok = writing
                      ? WriteFile(pipe, bytes + done, (DWORD)(size - done), &count, NULL)
                      : ReadFile(pipe, bytes + done, (DWORD)(size - done), &count, NULL);
        if (!ok) return -1;
        if (count == 0) return writing ? 1 : (done == 0 ? 0 : -1);
        done += (size_t)count;
    }
    return 1;
}

int32_t aurora_receive(uint8_t *bytes, size_t size) {
    if (size != (size_t)FRAME) return -1;
    if (open_pipe() != 1) return -1;
    return transfer(bytes, size, 0);
}

int32_t aurora_reply(uint8_t *bytes, size_t size) {
    if (size != (size_t)FRAME || open_pipe() != 1) return -1;
    return transfer(bytes, size, 1);
}
