/* Source-only memory proof, no original DLL/services/device.
 * cc -std=c11 -O1 -g -Wall -Wextra -Werror -fno-fast-math -ffp-contract=off
 * -fsanitize=address,undefined -fno-omit-frame-pointer
 * scripts/research/check_stereo_one_memory.c -lm -o CHECK && CHECK
 */
#include "../../core/native/effects/bridge.c"
#include <assert.h>
#include <stdio.h>
#define GUARD 16
static float *guarded(size_t n) {
    float *p=calloc(n+2*GUARD,sizeof(float));assert(p);
    for(size_t i=0;i<GUARD;i++){p[i]=123.5f;p[n+GUARD+i]=123.5f;}
    return p;
}
static void guard(const float *p,size_t n) {
    for(size_t i=0;i<GUARD;i++){assert(p[i]==123.5f);assert(p[n+GUARD+i]==123.5f);}
}
int main(void) {
    const unsigned rates[]={8000,44100,48000,96000,384000},capacities[]={4,8,64,256,512,2048,4096};
    for(unsigned r=0;r<5;r++)for(unsigned c=0;c<7;c++) {
        unsigned rate=rates[r],cap=capacities[c];size_t count=cumulus_ambience_one_storage(rate);
        float *arena=guarded(count),*pcm=guarded(2*cap),*dry=guarded(2*cap);
        NeteaseStereoOne *s=calloc(1,sizeof(*s)),*prior=calloc(1,sizeof(*s));assert(s && prior);
        assert(cumulus_stereo_one_init(s,rate,cap,arena+GUARD,count)==0);*prior=*s;
        assert(cumulus_stereo_one_init(s,rate,cap,arena+GUARD,count-1)!=0);
        assert(memcmp(prior,s,sizeof(*s))==0);
        assert(netease_stereo_one_parameters(s,NAN,0,1)!=0);
        assert(memcmp(prior,s,sizeof(*s))==0);
        unsigned n=0,end=0;
        assert(netease_stereo_one_process(s,pcm+GUARD,&n,&end,dry+GUARD)==0);
        assert(n==0 && end==0);
        for(unsigned k=0;k<5;k++)assert(s->shared.dynamics[0].detector[k].power>0.0f);
        for(unsigned b=0;b<64;b++) {
            n=b%6==0?0:b%6==1?1:b%6==2?cap-1:cap;end=b>=32;
            for(unsigned i=0;i<cap*2;i++)pcm[GUARD+i]=end?0.0f:(float)((int)((i+b)%13)-6)*0.015625f;
            assert(netease_stereo_one_parameters(s,(float)((int)(b%21)-10),(float)(b%11),b%4!=2)==0);
            assert(netease_stereo_one_bypass(s,b==5 || b==6 || b==16 || b==17)==0);
            if(b==13 || b==35)assert(netease_stereo_one_reset(s)==0);
            *prior=*s;
            unsigned invalid=n,invalid_end=end;
            assert(netease_stereo_one_process(s,pcm+GUARD,&invalid,&invalid_end,pcm+GUARD+1)!=0);
            assert(memcmp(prior,s,sizeof(*s))==0);
            assert(netease_stereo_one_process(s,pcm+GUARD,&n,&end,dry+GUARD)==0);
            assert(n<=cap && end<=1);
            for(unsigned i=0;i<cap*2;i++)assert(isfinite(pcm[GUARD+i]));
            guard(arena,count);guard(pcm,2*cap);guard(dry,2*cap);
        }
        free(prior);free(s);free(arena);free(pcm);free(dry);
    }
    puts("35 fixed-bank1 SE formats: zero-cache, overlap and ASan/UBSan guards passed");
    return 0;
}
