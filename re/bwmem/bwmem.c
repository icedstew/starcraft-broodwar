/* bwmem.exe: bwmem.exe <StarCraft.exe path>: starts 1.16.1 and reads its game
   state for re/record_glue.py. Runs under wine (winegcc): Linux blocks reading wine's
   memory from outside, and the game denies OpenProcess, so the handle CreateProcess
   returns is kept. Each line on stdin asks for one sample ("r <hex address>": a dword):
   "<0x6d11ec in game> <0x57f23c frame> <0x51ca10 seed> <0x62848c / 0x6284a8 screen x y>", or
   "-1 0 0 0 0" once it exited. */
#include <windows.h>
#include <stdio.h>

int main(int argc, char **argv) {
    /* On its own 640x480 virtual desktop (HKCU\Software\Wine\Explorer\Desktops "sc"): the
       game's mode change stays inside it. */
    char desktop[] = "sc";
    STARTUPINFOA si = { sizeof si };
    si.lpDesktop = desktop;
    PROCESS_INFORMATION pi;
    char line[16];
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 2 || !CreateProcessA(argv[1], NULL, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
        fprintf(stderr, "bwmem: CreateProcess failed (%lu)\n", GetLastError());
        return 1;
    }
    while (fgets(line, sizeof line, stdin)) {
        unsigned char in = 0; unsigned frame = 0, seed = 0, sx = 0, sy = 0, addr, v = 0; SIZE_T n;
        if (sscanf(line, "r %x", &addr) == 1) { /* "r <hex address>": that dword */
            ReadProcessMemory(pi.hProcess, (void *)(size_t)addr, &v, 4, &n);
            printf("%u\n", v);
            continue;
        }
        if (WaitForSingleObject(pi.hProcess, 0) == WAIT_OBJECT_0
            || !ReadProcessMemory(pi.hProcess, (void *)0x6d11ec, &in, 1, &n)) {
            puts("-1 0 0 0 0");
            continue;
        }
        ReadProcessMemory(pi.hProcess, (void *)0x57f23c, &frame, 4, &n);
        ReadProcessMemory(pi.hProcess, (void *)0x51ca10, &seed, 4, &n);
        ReadProcessMemory(pi.hProcess, (void *)0x62848c, &sx, 4, &n);
        ReadProcessMemory(pi.hProcess, (void *)0x6284a8, &sy, 4, &n);
        printf("%u %u %u %u %u\n", in, frame, seed, sx, sy);
    }
    return 0;
}
