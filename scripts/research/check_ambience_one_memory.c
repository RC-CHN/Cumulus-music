/* Standalone owned-arena safety check. No official DLL, account or device.
 * cc -std=c11 -O1 -g -fno-fast-math -ffp-contract=off
 *    -fsanitize=address,undefined -fno-omit-frame-pointer
 *    scripts/research/check_ambience_one_memory.c -lm -o CHECK && CHECK
 */
#include "../../core/native/effects/netease_ambience_one.c"
#include <assert.h>
#include <stdio.h>

#define GUARD 16
static void guard(const float *p,size_t n) {
    for(size_t i=0;i<GUARD;i++){assert(p[i]==123.5f);assert(p[n+GUARD+i]==123.5f);}
}
static float *guarded(size_t n) {
    float *p=calloc(n+2*GUARD,sizeof(float));assert(p);
    for(size_t i=0;i<GUARD;i++){p[i]=123.5f;p[n+GUARD+i]=123.5f;}
    return p;
}
int main(void) {
    const uint32_t rates[]={8000,44100,48000,96000,384000};
    const uint32_t capacities[]={4,8,64,256,512,2048,4096};
    for(unsigned r=0;r<5;r++)for(unsigned c=0;c<7;c++) {
        uint32_t rate=rates[r],cap=capacities[c];size_t count=cumulus_ambience_one_storage(rate);
        assert(count && count<=AMB1_STORAGE_LIMIT);
        float *arena=guarded(count),*pcm=guarded(2*cap);
        struct {uint64_t before;NeteaseAmbienceOne state;uint64_t after;} owned;
        memset(&owned,0,sizeof(owned));owned.before=0x12345678;owned.after=0xabcdef01;
        NeteaseAmbienceOne *s=&owned.state;
        assert(cumulus_ambience_one_init(s,rate,cap,arena+GUARD,count)==0);
        NeteaseAmbienceOne initial=*s;
        /* Reject before touching state or the arena; only Rust-owned, disjoint
         * storage is passed. The supported PCM API is intentionally in place. */
        assert(cumulus_ambience_one_init(s,rate,cap,arena+GUARD,count-1)!=0);
        assert(memcmp(s,&initial,sizeof(*s))==0);
        assert(cumulus_ambience_one_init(s,rate,cap+1,arena+GUARD,count)!=0);
        assert(memcmp(s,&initial,sizeof(*s))==0);
        for(unsigned b=0;b<64;b++) {
            uint32_t n=b%7==0?0:b%7==1?1:b%7==2?cap-1:cap,end=b>=32;
            float *x=pcm+GUARD;memset(x,0,cap*2*sizeof(float));
            if(b<32)for(unsigned i=0;i<2*cap;i++)x[i]=(float)((int)(i%13)-6)*0.015625f;
            if(b==17 || b==35) {
                uint32_t before[87],after[87];float h[70];
                assert(netease_ambience_one_snapshot(s,before,h)==0);
                assert(netease_ambience_one_reset(s)==0);
                assert(netease_ambience_one_snapshot(s,after,h)==0);
                assert(memcmp(before,after,sizeof(before))==0);
                for(unsigned i=0;i<16;i++)for(unsigned j=0;j<4;j++)assert(h[6+4*i+j]==0.0f);
            }
            assert(netease_ambience_one_process(s,x,&n,&end)==0);
            assert(n<=cap && end<=1);
            guard(arena,count);guard(pcm,2*cap);
            assert(owned.before==0x12345678 && owned.after==0xabcdef01);
        }
        /* NaN input is an atomic validation error, not an accepted update. */
        NeteaseAmbienceOne prior=*s;uint32_t n=1,end=0;
        pcm[GUARD]=NAN;
        assert(netease_ambience_one_process(s,pcm+GUARD,&n,&end)!=0);
        assert(memcmp(s,&prior,sizeof(*s))==0);
        guard(arena,count);guard(pcm,2*cap);free(arena);free(pcm);
    }
    assert(cumulus_ambience_one_storage(7999)==0);
    assert(cumulus_ambience_one_storage(384001)==0);
    puts("35 rate/capacity owned-arena cases: ASan/UBSan and guards passed");
    return 0;
}
