// Runs the loop of animated_smiles.ino on FastEPD's own drawing code (memory-only mode, a virtual
// 960 x 540 panel) and prints a hash of the framebuffer after 1, 10, 100 and 250 frames, and the
// random numbers it used. `tests/animated_smiles_ui.rs` feeds the same numbers to the Rust port and
// compares. Rebuild and run (FastEPD `main` checked out at ~/Src/tmp/FastEPD, FastEPD 60d4945):
//
//   clang++ -std=c++17 -O1 -w -I ~/Src/tmp/FastEPD/src fastepd_frames.cpp -o fastepd_frames && ./fastepd_frames 1
//
// The frame dumps it also writes (frame*.pgm) can be viewed with any PGM reader.
// The loop of animated_smiles.ino, run on FastEPD's own drawing code in memory-only mode.
#define __MEM_ONLY__
#define __LINUX__
#include "/Users/mordor/Src/tmp/FastEPD/src/FastEPD.cpp"
#include "/Users/mordor/Src/tmp/FastEPD/examples/Arduino/animated_smiles/smiley.h"
#include <stdlib.h>
#include <stdio.h>

FASTEPD epaper;
#define NUM_SMILEYS 8
int xArray[NUM_SMILEYS], yArray[NUM_SMILEYS], xDir[NUM_SMILEYS], yDir[NUM_SMILEYS];
static unsigned long raw[64]; static int nraw = 0;
static long arduino_random(long lo, long hi) { unsigned long r = (unsigned long)random(); raw[nraw++] = r; return lo + r % (hi - lo); }
// FNV-1a 64 over one byte per pixel, 255 for white and 0 for black, row by row.
static void report(int frames) {
    uint8_t *buf = epaper.currentBuffer();
    int w = epaper.width(), h = epaper.height(), pitch = (w + 7) / 8;
    unsigned long long hash = 14695981039346656037ULL; long black = 0;
    for (int y = 0; y < h; y++) for (int x = 0; x < w; x++) {
        int white = (buf[y * pitch + (x >> 3)] & (0x80 >> (x & 7))) != 0;
        hash ^= white ? 255 : 0; hash *= 1099511628211ULL; black += !white;
    }
    printf("frames %d black %ld hash 0x%016llx\n", frames, black, hash);
}

static void dump(const char *name) {
    uint8_t *buf = epaper.currentBuffer();
    int w = epaper.width(), h = epaper.height(), pitch = (w + 7) / 8;
    FILE *f = fopen(name, "wb");
    fprintf(f, "P5\n%d %d\n255\n", w, h);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++)
            fputc((buf[y * pitch + (x >> 3)] & (0x80 >> (x & 7))) ? 255 : 0, f);
    fclose(f);
}

int main(int argc, char **argv) {
    srandom(argc > 1 ? atoi(argv[1]) : 1);
    epaper.initPanel(BB_PANEL_VIRTUAL);
    epaper.setPanelSize(960, 540);
    epaper.fillScreen(BBEP_WHITE); // what clearWhite() leaves in the buffer
    fprintf(stderr, "panel %d x %d, mode %d\n", epaper.width(), epaper.height(), epaper.getMode());
    for (int i = 0; i < NUM_SMILEYS; i++) {
        xArray[i] = arduino_random(4, epaper.width() - 104);
        yArray[i] = arduino_random(4, epaper.height() - 104);
        xDir[i] = arduino_random(0, 2) ? -4 : 4;
        yDir[i] = arduino_random(0, 2) ? -4 : 4;
    }
    for (int j = 0; j < 250; j++) {
        for (int i = 0; i < NUM_SMILEYS; i++) {
            int rc = epaper.loadG5Image(smiley, xArray[i], yArray[i], BBEP_WHITE, BBEP_BLACK, 1.0f);
            if (rc != BBEP_SUCCESS) { fprintf(stderr, "loadG5Image rc %d\n", rc); return 1; }
            xArray[i] += xDir[i];
            if (xArray[i] < 4 || xArray[i] > epaper.width() - 104) xDir[i] = -xDir[i];
            yArray[i] += yDir[i];
            if (yArray[i] < 4 || yArray[i] > epaper.height() - 104) yDir[i] = -yDir[i];
        }
        if (j == 0 || j == 9 || j == 99) report(j + 1);
        if (j == 0) dump("frame000.pgm");
        if (j == 9) dump("frame010.pgm");
        if (j == 99) dump("frame100.pgm");
    }
    dump("frame250.pgm");
    report(250);
    printf("raw");
    for (int i = 0; i < nraw; i++) printf(" %lu", raw[i]);
    printf("\n");
    return 0;
}
