// Feeds LibAtrac9 random superframes the way the Rust wrapper does (each copied into a
// zero-padded buffer), for several stream configurations, to find crashes with the sanitizers:
//   gcc -fsanitize=address,undefined -g -O1 -I../libatrac9 ../libatrac9/*.c noise.c -lm -o noise
//   ./noise [rounds per configuration]
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "libatrac9.h"

int main(int argc, char **argv) {
    int rounds = argc > 1 ? atoi(argv[1]) : 20000;
    // 48 kHz stereo (a real theme song), 48 kHz mono, 44.1 kHz stereo, 96 kHz stereo, and a
    // larger stereo frame: each byte 1 is sample rate (4 bits), channel layout (3), 0.
    const unsigned char configs[][4] = {
        {0xFE, 0x74, 0x05, 0xF0}, {0xFE, 0x70, 0x05, 0xF0}, {0xFE, 0x64, 0x05, 0xF0},
        {0xFE, 0xC4, 0x05, 0xF0}, {0xFE, 0x74, 0x17, 0xF0},
    };
    static unsigned char buffer[8192 + 4096];
    static short pcm[8 * 1024];
    unsigned int seed = 0x2545F491u;
    for (unsigned c = 0; c < sizeof configs / sizeof configs[0]; c++) {
        unsigned char config[4];
        memcpy(config, configs[c], 4);
        void *handle = Atrac9GetHandle();
        Atrac9CodecInfo info;
        if (Atrac9InitDecoder(handle, config) != 0 || Atrac9GetCodecInfo(handle, &info) != 0) {
            printf("config %u refused\n", c);
            Atrac9ReleaseHandle(handle);
            continue;
        }
        int size = info.superframeSize;
        for (int round = 0; round < rounds; round++) {
            for (int i = 0; i < size; i++) {
                seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
                buffer[i] = (unsigned char)seed;
            }
            memset(buffer + size, 0, 4096);
            int offset = 0;
            for (int frame = 0; frame < info.framesInSuperframe && offset < size; frame++) {
                int used = 0;
                if (Atrac9Decode(handle, buffer + offset, pcm, &used) != 0) break;
                offset += used;
            }
        }
        printf("config %u (%d Hz, %d ch, %d-byte superframes): %d rounds\n", c, info.samplingRate,
               info.channels, size, rounds);
        Atrac9ReleaseHandle(handle);
    }
    puts("no crash");
    return 0;
}
