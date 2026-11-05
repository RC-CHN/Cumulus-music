/* Manual Linux synthetic ASan/UBSan witness; no original DLL or accounts. */
#include "../../core/native/effects/bridge.c"
#include <stdlib.h>
#include <assert.h>
#include <stdio.h>
int main(void) {
 const unsigned rates[]={44100,48000,96000},caps[]={512,2048};unsigned calls=0;
 for(unsigned r=0;r<3;r++)for(unsigned k=0;k<2;k++) {
  unsigned rate=rates[r],cap=caps[k];size_t count=cumulus_ambience_one_storage(rate);
  NeteaseLocalFixed *s=aligned_alloc(16,(sizeof(*s)+15)&~(size_t)15);
  float *arena=calloc(count,sizeof(float)),*pcm=calloc(2*cap,sizeof(float)),*dry=calloc(2*cap,sizeof(float));assert(s&&arena&&pcm&&dry);
  assert(cumulus_local_fixed_init(s,rate,cap,arena,count)==0);
  assert(netease_local_fixed_parameters(s,NAN,5,1)==-1);
  unsigned n=cap+1,end=0;assert(netease_local_fixed_process(s,pcm,&n,&end,dry)==-1);
  n=7;assert(netease_local_fixed_process(s,pcm,&n,&end,pcm+1)==-1);
  assert(netease_local_fixed_parameters(s,3,5,1)==0);
  for(unsigned b=0;b<220;b++) {
   for(unsigned i=0;i<2*cap;i++)pcm[i]=i<7?(float)(i+1)*.01f:0;
   n=b==0?7:0;end=b>=1;
   assert(netease_local_fixed_process(s,pcm,&n,&end,dry)==0);assert(n<=cap&&end<=1);
   for(unsigned i=0;i<2*cap;i++){assert(isfinite(pcm[i]));}
   calls++;
  }
  assert(netease_local_fixed_reset(s)==0);
  assert(netease_local_fixed_parameters(s,0,0,0)==0);
  n=0;end=1;assert(netease_local_fixed_process(s,pcm,&n,&end,dry)==0);calls++;
  free(dry);free(pcm);free(arena);free(s);
 }
 printf("fixed local memory: %u calls; 3 rates x 2 caps; ASan/UBSan\n",calls);return 0;
}
