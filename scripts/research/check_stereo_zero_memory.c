/* Optional sanitizer test of the portable C component. No DLL/assets/audio. */
#include "../../core/native/effects/bridge.c"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
static void guards(const float *p,unsigned count) {
    for(unsigned i=0;i<4;i++)assert(p[i]==-9876.5f && p[count+4+i]==-9876.5f);
}
static uint64_t state_hash(const NeteaseStereoZero *s) {
    const unsigned char *p=(const unsigned char *)s;
    uint64_t hash=1469598103934665603ULL;
    for(size_t i=0;i<sizeof(*s);i++)hash=(hash^p[i])*1099511628211ULL;
    return hash;
}
int main(void) {
    unsigned blocks=0;
    const unsigned capacities[5]={4,8,64,512,4096};
    const float rates[3]={44100.0f,48000.0f,96000.0f};
    for(unsigned r=0;r<3;r++)for(unsigned c=0;c<5;c++) {
        unsigned capacity=capacities[c],count=2*capacity;
        NeteaseStereoZero *state=aligned_alloc(16,(sizeof(*state)+15)&~(size_t)15);
        float *audio=calloc(count+8,sizeof(float)),*scratch=calloc(count+8,sizeof(float));
        float *unchanged=malloc(count*sizeof(float));
        assert(state && audio && scratch && unchanged);
        for(unsigned i=0;i<count+8;i++){audio[i]=-9876.5f;scratch[i]=-9876.5f;}
        assert(cumulus_stereo_zero_init(state,rates[r],capacity)==0);
        for(unsigned step=0;step<20;step++) {
            unsigned frames=step%3==0?1:step%3==1?capacity-1:capacity;
            float before[22],after[22];
            uint64_t original=state_hash(state);
            memcpy(unchanged,audio+4,count*sizeof(float));
            assert(netease_stereo_zero_history(state,before)==0);
            assert(netease_stereo_zero_parameters(state,NAN,0.0f,1)==-1);
            assert(netease_stereo_zero_parameters(state,0.0f,INFINITY,1)==-1);
            assert(netease_stereo_zero_parameters(state,0.0f,0.0f,2)==-1);
            assert(netease_stereo_zero_history(state,after)==0 && memcmp(before,after,sizeof(before))==0);
            assert(netease_stereo_zero_process(state,audio+4,0,scratch+4)==-1);
            assert(netease_stereo_zero_process(state,audio+4,capacity+1,scratch+4)==-1);
            assert(netease_stereo_zero_process(state,audio+4,frames,audio+5)==-1);
            assert(netease_stereo_zero_history(state,after)==0 && memcmp(before,after,sizeof(before))==0);
            assert(state_hash(state)==original);
            assert(memcmp(unchanged,audio+4,count*sizeof(float))==0);
            for(unsigned i=0;i<count;i++)audio[4+i]=0.4f*sinf((float)(i+step)*0.037f);
            assert(netease_stereo_zero_parameters(state,(float)(step%21)-10.0f,(float)(step%11),step%2)==0);
            assert(netease_stereo_zero_bypass(state,step%4==0)==0);
            if(step%5==0)assert(netease_stereo_zero_reset(state)==0);
            assert(netease_stereo_zero_process(state,audio+4,frames,scratch+4)==0);
            for(unsigned i=0;i<count;i++)assert(isfinite(audio[4+i]));
            guards(audio,count);guards(scratch,count);blocks++;
        }
        free(unchanged);free(audio);free(scratch);free(state);
    }
    printf("{\"formats\":15,\"blocks\":%u,\"guards\":true,\"overlapRejected\":true}\n",blocks);
    return 0;
}
