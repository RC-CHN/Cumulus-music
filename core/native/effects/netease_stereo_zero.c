/* Reachable StereoEnhancer ambience=0 path, neaudioeffects 3.1.42 x64.
 * B28E0/B3060, A80D0/A8550 and A85B0. This is a source translation of the
 * verified branch, not an environment/reverb implementation or full graph.
 * Included after netease_multiband.c; all child states are owned in this struct.
 */
int cumulus_multiband_init(NeteaseMultiband *,float,unsigned,const unsigned char *,size_t);
#ifdef _MSC_VER
#pragma warning(push)
#pragma warning(disable: 4324)
#endif
typedef struct {
    NeteasePeq eq[4];
    NeteaseMultiband dynamics[2];
    unsigned capacity,bypass,previous,shaper,presence_dirty;
    float presence,stereoizer,side_gain,presence_gain;
    _Alignas(16) float side[8192],eq_dry[4096];
} NeteaseStereoZero;
#ifdef _MSC_VER
#pragma warning(pop)
#endif

static void stereo_zero_uint(unsigned char *p,uint32_t value) {
    for(unsigned i=0;i<4;i++)p[i]=(unsigned char)(value>>(8*i));
}
static void stereo_zero_float(unsigned char *p,float value) {
    stereo_zero_uint(p,float_bits(value));
}
/* Typed bank-0 constants, not an embedded opaque bank/resource. This is the
 * two 171-byte child schema already recovered from C1D00. */
static void stereo_zero_dynamics(unsigned char out[171],unsigned side) {
    static const float attack[5]={0.5f,0.4f,0.3f,0.2f,0.1f};
    static const float release[5]={1.0f,0.8f,0.3f,0.4f,0.2f};
    static const float crossover[4]={100.0f,350.0f,1500.0f,7500.0f};
    memset(out,0,171);
    for(unsigned band=0;band<5;band++) {
        const float row[7]={0.0f,side?-27.0f:-22.0f,1.2f,attack[band],
            side && band==2?0.6f:release[band],-6.0f,2.0f};
        for(unsigned field=0;field<7;field++)stereo_zero_float(out+28*band+4*field,row[field]);
    }
    for(unsigned i=0;i<4;i++)stereo_zero_float(out+140+4*i,crossover[i]);
    out[160]=1;out[161]=1;
    stereo_zero_uint(out+163,5);stereo_zero_float(out+167,side?1.5f:0.0f);
}
int netease_stereo_zero_reset(NeteaseStereoZero *s) {
    if(!s || !s->capacity)return -1;
    for(unsigned eq=0;eq<4;eq++)for(unsigned band=0;band<9;band++)
        netease_biquad_reset(&s->eq[eq].filters[0][band]);
    for(unsigned channel=0;channel<2;channel++)
        if(netease_multiband_reset(&s->dynamics[channel]))return -1;
    /* B3990 preserves the parent side gain and applied presence increment,
     * as well as the children's prior gains; reset is not reinitialization. */
    return 0;
}
int cumulus_stereo_zero_init(NeteaseStereoZero *s,float rate,unsigned capacity) {
    if(!s || !isfinite(rate) || rate<8000.0f || rate>384000.0f ||
       rate!=(float)(unsigned)rate || capacity<4 || capacity>4096 || capacity%4)return -1;
    memset(s,0,sizeof(*s));s->capacity=capacity;s->shaper=1;
    for(unsigned eq=0;eq<4;eq++) {
        NeteasePeq *child=&s->eq[eq];
        if(netease_peq_init(child,rate,1,capacity,0,1))return -1;
        if(eq%2==0) {
            if(netease_peq_band(child,0,7,-5.0f,400.0f,0.4f,1) ||
               netease_peq_band(child,1,6,24.0f,864.0f,1.0f,0) ||
               netease_peq_band(child,2,8,1.5f,5000.0f,0.4f,1))return -1;
        } else {
            if(netease_peq_band(child,0,4,0.0f,100.0f,1.0f,0) ||
               netease_peq_band(child,1,6,0.0f,eq==1?4000.0f:1000.0f,1.0f,0) ||
               netease_peq_band(child,2,5,0.0f,12000.0f,1.0f,0))return -1;
        }
    }
    for(unsigned channel=0;channel<2;channel++) {
        unsigned char raw[171];stereo_zero_dynamics(raw,channel);
        if(cumulus_multiband_init(&s->dynamics[channel],rate,capacity,raw,sizeof(raw)))return -1;
    }
    /* A82B0 initializes bank 1 before A85B0 selects ambience/bank 0. Its only
     * differing gain target is side-input 1 dB -> 1.5 dB. B3990 preserves the
     * old linear gain, so the first enabled shaper block must ramp from 1 dB. */
    s->dynamics[1].input=db_gain(1.0f);
    return netease_stereo_zero_reset(s);
}
int netease_stereo_zero_parameters(NeteaseStereoZero *s,float presence,float stereoizer,unsigned shaper) {
    if(!s || !s->capacity || !isfinite(presence) || !isfinite(stereoizer) ||
       presence < -10.0f || presence > 10.0f || stereoizer < 0.0f || stereoizer > 10.0f || shaper>1)return -1;
    if(presence!=s->presence){s->presence=presence;s->presence_dirty=1;}
    s->stereoizer=stereoizer;s->shaper=shaper;return 0;
}
int netease_stereo_zero_bypass(NeteaseStereoZero *s,unsigned bypass) {
    if(!s || !s->capacity || bypass>1)return -1;
    if(!bypass && s->bypass && netease_stereo_zero_reset(s))return -1;
    s->bypass=bypass;return 0;
}
int netease_stereo_zero_history(const NeteaseStereoZero *s,float out[22]) {
    if(!s || !s->capacity || !out)return -1;
    out[0]=s->side_gain;out[1]=s->presence_gain;
    for(unsigned channel=0;channel<2;channel++)
        if(netease_multiband_history(&s->dynamics[channel],out+2+channel*10))return -1;
    return 0;
}
int netease_stereo_zero_process(NeteaseStereoZero *s,float *planar,size_t frames,float *dry) {
    if(!s || !s->capacity || !planar || !dry || !frames || frames>s->capacity)return -1;
    size_t bytes=2*(size_t)s->capacity*sizeof(float);
    uintptr_t a=(uintptr_t)planar,b=(uintptr_t)dry;
    if(a>UINTPTR_MAX-bytes || b>UINTPTR_MAX-bytes || (a<b+bytes && b<a+bytes))return -1;
    unsigned changing=s->bypass!=s->previous;
    if(changing)memcpy(dry,planar,bytes);
    if(changing || !s->bypass) {
        unsigned csr=begin_plugin();
        if(s->presence_dirty) {
            float applied=s->presence*0.60000002384185791015625f;
            float difference=applied-s->presence_gain;
            if(difference!=0.0f) {
                NeteasePeqBand p=s->eq[1].bands[1];
                netease_peq_band(&s->eq[1],1,p.type,p.gain+difference,p.frequency,p.q,p.enabled);
                s->presence_gain=applied;
            }
            s->presence_dirty=0;
        }
        for(size_t i=0;i<frames;i++) {
            float left=planar[i],right=planar[s->capacity+i];
            planar[i]=left+right;planar[s->capacity+i]=left-right;
        }
        float target=powf(10.0f,s->stereoizer*0.05000000074505806f);
        netease_peq_gain(planar+s->capacity,frames,s->side_gain,target);s->side_gain=target;
        if(s->shaper)for(unsigned channel=0;channel<2;channel++) {
            float *side=s->side+channel*s->capacity;
            memcpy(side,planar+channel*s->capacity,frames*sizeof(float));
            netease_peq_process(&s->eq[channel*2],side,frames,s->eq_dry);
            netease_multiband_process(&s->dynamics[channel],planar+channel*s->capacity,side,frames);
        }
        for(unsigned channel=0;channel<2;channel++)
            netease_peq_process(&s->eq[channel*2+1],planar+channel*s->capacity,frames,s->eq_dry);
        size_t vector=frames&~(size_t)3;
        for(size_t i=0;i<frames;i++) {
            float mid=planar[i],side=planar[s->capacity+i];
            planar[i]=i<vector?mid*0.5f+side*0.5f:(mid+side)*0.5f;
            planar[s->capacity+i]=(mid-side)*0.5f;
        }
        end_plugin(csr);
    }
    if(changing)blend_channels(planar,dry,2,s->capacity,s->bypass);
    s->previous=s->bypass;return 0;
}
