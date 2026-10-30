#include <stdint.h>
#include <string.h>
#include <stdio.h>
#define PROGMEM
#define pgm_read_byte(p) (*(const uint8_t*)(p))
#define pgm_read_word(p) (*(const uint16_t*)(p))
#include "Group5.h"
#include "g5dec.inl"
#include "/Users/mordor/Src/Arduino/M5PaperDIY/FastEPD/animated_smiles/smiley.h"
int main(){
  const uint8_t *p = smiley;
  uint16_t w=p[2]|p[3]<<8, h=p[4]|p[5]<<8, sz=p[6]|p[7]<<8;
  fprintf(stderr,"marker %02x%02x w %d h %d size %d\n",p[0],p[1],w,h,sz);
  static G5DECIMAGE g; uint8_t line[64];
  int rc=g5_decode_init(&g,w,h,(uint8_t*)p+8,sz); fprintf(stderr,"init rc %d\n",rc);
  for(int y=0;y<h;y++){ memset(line,0,sizeof line); g5_decode_line(&g,line);
    for(int x=0;x<w;x++) putchar((line[x>>3]&(0x80>>(x&7)))?'#':'.'); putchar('\n'); }
}
