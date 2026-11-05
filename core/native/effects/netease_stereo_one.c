/* Fixed bank1 SE parent: B28E0 + A80D0/A8550, real child EOF/reset.
 * Included after the shared SE0 and bank1 environment source. No full material
 * graph, arbitrary bank, live engine, UI or entitlement is implied. */
#ifdef _MSC_VER
#pragma warning(push)
#pragma warning(disable: 4324)
#endif
typedef struct {
    NeteaseStereoZero shared;
    NeteaseAmbienceOne environment;
    _Alignas(16) float environment_input[8192];
} NeteaseStereoOne;
#ifdef _MSC_VER
#pragma warning(pop)
#endif
int cumulus_stereo_one_init(NeteaseStereoOne *s,uint32_t rate,unsigned cap,float *arena,size_t count) {
    if(!s || !arena || count!=cumulus_ambience_one_storage(rate) || !count ||
       cap<4 || cap>4096 || cap%4)return -1;
    memset(s,0,sizeof(*s));
    if(cumulus_stereo_zero_init(&s->shared,(float)rate,cap))return -1;
    /* A82B0 starts in bank1, so both the old and target side-input gain are
     * 1dB. SE0's later selection of bank0 changes only its target to 1.5dB. */
    s->shared.dynamics[1].config.input=db_gain(1.0f);
    return cumulus_ambience_one_init(&s->environment,rate,cap,arena,count);
}
int netease_stereo_one_reset(NeteaseStereoOne *s) {
    if(!s || !s->shared.capacity)return -1;
    if(netease_stereo_zero_reset(&s->shared))return -1;
    return netease_ambience_one_reset(&s->environment);
}
int netease_stereo_one_bypass(NeteaseStereoOne *s,unsigned bypass) {
    if(!s || !s->shared.capacity || bypass>1)return -1;
    if(!bypass && s->shared.bypass && netease_stereo_one_reset(s))return -1;
    s->shared.bypass=bypass;return 0;
}
int netease_stereo_one_parameters(NeteaseStereoOne *s,float presence,float stereoizer,unsigned shaper) {
    return !s?-1:netease_stereo_zero_parameters(&s->shared,presence,stereoizer,shaper);
}
int netease_stereo_one_history(const NeteaseStereoOne *s,float out[22]) {
    return !s?-1:netease_stereo_zero_history(&s->shared,out);
}
int netease_stereo_one_environment(const NeteaseStereoOne *s,uint32_t *indices,float *history) {
    return !s?-1:netease_ambience_one_snapshot(&s->environment,indices,history);
}
int netease_stereo_one_process(NeteaseStereoOne *whole,float *planar,unsigned *frames,unsigned *end,float *dry) {
    if(!whole || !whole->shared.capacity || !planar || !frames || !end || !dry)return -1;
    NeteaseStereoZero *s=&whole->shared;size_t n=*frames;
    if(n>s->capacity || *end>1)return -1;
    uintptr_t a=(uintptr_t)planar,b=(uintptr_t)dry;
    size_t bounded=2*(size_t)s->capacity*sizeof(float);
    if(a>UINTPTR_MAX-bounded || b>UINTPTR_MAX-bounded || (a<b+bounded && b<a+bounded))return -1;
    size_t bytes=2*(size_t)s->capacity*sizeof(float);
    unsigned changing=s->bypass!=s->previous;
    if(changing)memcpy(dry,planar,bytes);
    if(changing || !s->bypass) {
        unsigned csr=begin_plugin();
        if(stereo_shared_prepare(s,planar,n,1)){end_plugin(csr);return -2;}
        memcpy(whole->environment_input,planar+s->capacity,n*sizeof(float));
        memcpy(whole->environment_input+s->capacity,planar+s->capacity,n*sizeof(float));
        unsigned returned=(unsigned)n,finished=*end;
        if(netease_ambience_one_process(&whole->environment,whole->environment_input,&returned,&finished)){end_plugin(csr);return -3;}
        size_t vector=n&~(size_t)3;
        for(size_t i=0;i<n;i++) {
            float mid=planar[i],side=planar[s->capacity+i];
            float left=i<vector?side*0.5f+mid*0.5f:(side+mid)*0.5f;
            float right=(mid-side)*0.5f;
            planar[i]=left+whole->environment_input[i];
            planar[s->capacity+i]=right+whole->environment_input[s->capacity+i];
        }
        for(size_t i=n;i<returned;i++) {
            planar[i]=whole->environment_input[i];
            planar[s->capacity+i]=whole->environment_input[s->capacity+i];
        }
        *frames=returned;*end=finished;end_plugin(csr);
    }
    if(changing)blend_channels(planar,dry,2,s->capacity,s->bypass);
    s->previous=s->bypass;return 0;
}
