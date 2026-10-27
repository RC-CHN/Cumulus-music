/* Mono multiband dynamics used by StereoEnhancer. Original RVA C2B20/C3020,
 * C7060/C7340 crossovers and BFB50/BFC90 detectors. No original machine code.
 * Compile this translation unit alone: it includes the recovered filter core.
 */
#include "netease_dsp.c"

typedef struct {float input,threshold,ratio,attack,release,floor,output;} NeteaseDynamicsBand;
typedef struct {
    NeteaseDynamicsBand band[5];float frequency[4],output;
    unsigned lfe,linked,fourth,count;float input;
} NeteaseDynamicsConfig;
typedef struct {NeteaseBiquad filters[22];unsigned fourth;} NeteaseCrossover;
typedef struct {float envelope,power;} NeteaseDetector;
#ifdef _MSC_VER
/* The padding is intentional: SIMD data stays 16-byte aligned. The FFI queries
 * the actual native size/alignment; never pack this struct to silence C4324. */
#pragma warning(push)
#pragma warning(disable: 4324)
#endif
typedef struct {
    float rate;unsigned capacity,initialized;
    NeteaseDynamicsConfig config;NeteaseCrossover crossover[2];NeteaseDetector detector[5];
    float input,output,band_input[5],band_output[5],rms;
    _Alignas(16) float data[2][5][4096];
} NeteaseMultiband;
#ifdef _MSC_VER
#pragma warning(pop)
#endif

static uint32_t read_uint(const unsigned char *p) {
    return (uint32_t)p[0]|((uint32_t)p[1]<<8)|((uint32_t)p[2]<<16)|((uint32_t)p[3]<<24);
}
static float read_float(const unsigned char *p){return from_bits(read_uint(p));}
static float db_gain(float x){return powf(10.0f,x*0.05000000074505806f);}

/* Serialized blocks store dB for input/output gains; the plugin stores linear
 * values after C1D00. A bad block is rejected before modifying live state. */
static int dynamics_decode(NeteaseDynamicsConfig *out,const unsigned char *raw,size_t bytes) {
    if(!raw || bytes!=171)return -1;
    for(unsigned i=0;i<40;i++)if(!isfinite(read_float(raw+4*i)))return -1;
    if(!isfinite(read_float(raw+167)) || raw[160]>1 || raw[161]>1 || raw[162]>1)return -1;
    uint32_t count=read_uint(raw+163);if(count<1 || count>5)return -1;
    NeteaseDynamicsConfig c={0};
    for(unsigned i=0;i<5;i++) {
        float v[7];for(unsigned j=0;j<7;j++)v[j]=read_float(raw+28*i+4*j);
        if(fabsf(v[0])>120 || fabsf(v[6])>120 || v[2]<=0 || v[2]>100 ||
           v[3]<=0 || v[4]<=0 || v[3]>100 || v[4]>100 || fabsf(v[1])>200 || fabsf(v[5])>200)return -1;
        c.band[i]=(NeteaseDynamicsBand){db_gain(v[0]),v[1],v[2],v[3],v[4],v[5],db_gain(v[6])};
    }
    for(unsigned i=0;i<4;i++) {
        c.frequency[i]=read_float(raw+140+4*i);
        if(c.frequency[i]<=0 || c.frequency[i]>384000)return -1;
    }
    float output=read_float(raw+156),input=read_float(raw+167);
    if(fabsf(output)>120 || fabsf(input)>120)return -1;
    c.output=db_gain(output);c.input=db_gain(input);c.lfe=raw[160];c.linked=raw[161];
    c.fourth=raw[162];c.count=count;*out=c;return 0;
}

/* C7F30's second-order crossover uses an inverted high branch. Its phase
 * compensation is a first-order allpass from C7660; fourth-order mode uses
 * two Butterworth sections plus second-order allpasses. */
int netease_crossover_coefficients(unsigned kind,unsigned fourth,float rate,float frequency,float c[5]) {
    if(kind>2 || fourth>1 || !c || !isfinite(rate) || rate<8000 || rate>384000 ||
       !isfinite(frequency) || frequency<=0)return -1;
    float w=(fminf(rate*0.44999998807907104f,frequency)*3.1415927410125732f)/rate;
    if(kind==2) {
        if(!fourth) {
            float a=-1.0f/tanf(w+0.7853981852531433f);
            c[0]=a;c[1]=1;c[2]=0;c[3]=-a;c[4]=0;
        } else {
            float t=1.0f/tanf(w),sq=t*t,term=t*1.4142135381698608f;
            float total=sq+1.0f,inv=1.0f/(total+term);
            float a=(total-term)*inv,b=((sq-1.0f)*-2.0f)*inv;
            c[0]=a;c[1]=b;c[2]=1;c[3]=-b;c[4]=-a;
        }
    } else if(fourth) {
        return netease_biquad_coefficients(kind?4:3,rate,frequency,0,1,c);
    } else {
        float phase=w+0.7853981852531433f;
        float sine=filter_sine(phase),cosine=filter_sine(phase+1.5707963705062866f);
        float inv=1.0f/(sine*sine);
        float t=kind?cosf(w):sinf(w),center=(t*t)*inv;
        float edge=center*(kind?-0.5f:0.5f);
        float a1=-(cosf(w+w)*inv),a2=(cosine*cosine)*inv;
        c[0]=edge;c[1]=center;c[2]=edge;c[3]=-a1;c[4]=-a2;
    }
    return 0;
}

static void crossover_configure(NeteaseCrossover *s,float rate,const float frequency[4]) {
    unsigned index=0;
    for(unsigned band=0;band<4;band++) {
        float c[5];netease_crossover_coefficients(0,s->fourth,rate,frequency[band],c);
        netease_biquad_set(&s->filters[index++],c);
        if(s->fourth)netease_biquad_set(&s->filters[index++],c);
        for(unsigned higher=band+1;higher<4;higher++) {
            netease_crossover_coefficients(2,s->fourth,rate,frequency[higher],c);
            netease_biquad_set(&s->filters[index++],c);
        }
        netease_crossover_coefficients(1,s->fourth,rate,frequency[band],c);
        netease_biquad_set(&s->filters[index++],c);
        if(s->fourth)netease_biquad_set(&s->filters[index++],c);
    }
}

static void crossover_process(NeteaseCrossover *s,float (*data)[4096],size_t n,unsigned count) {
    unsigned index=0;
    for(unsigned band=0;band+1<count;band++) {
        memcpy(data[band+1],data[band],n*4);
        unsigned sections=count-band-1+s->fourth;
        for(unsigned i=0;i<sections;i++)netease_biquad_process(&s->filters[index++],data[band],n);
        index+=5-count;
        netease_biquad_process(&s->filters[index++],data[band+1],n);
        if(s->fourth)netease_biquad_process(&s->filters[index++],data[band+1],n);
    }
}

static float detector_log(float power) {
    uint32_t bits=float_bits(power);float mantissa=from_bits((bits&0x7fffff)+0x3f800000);
    float z=(mantissa-1.0f)/(mantissa+1.0f);
    float cubic=((z*z)*z)*2.895296812057495f;
    float linear=z*8.685890197753906f;
    float exponent=((float)((bits>>23)&255)-127.0f)*3.0102999210357666f;
    return (cubic+linear)+exponent;
}

static void detector_process(NeteaseDetector *s,float *data,const float *side,size_t n,
                             float rate,float rms,NeteaseDynamicsBand p) {
    float attack=expf(-2.200000047683716f/(p.attack*rate));
    float release=expf(-2.200000047683716f/(p.release*rate));
    float slope=1.0f/p.ratio-1.0f;
    for(size_t i=0;i<n;i++) {
        /* C0650 deliberately keeps the external detector pointer unchanged
         * within a block. The ordinary detector follows the current sample. */
        float x=side?side[0]:data[i],power=x*x+1.0000000195414814e-25f;
        s->power=(s->power-power)*rms+power;
        float over=fmaxf(detector_log(s->power)-p.threshold,0.0f);
        float smoothing=(over-s->envelope)<0?release:attack;
        s->envelope=(s->envelope-over)*smoothing+over;
        float reduction=fmaxf(s->envelope*slope,p.floor)*0.05000000074505806f;
        data[i]=data[i]*fast_power10(reduction);
    }
}

size_t netease_multiband_size(void){return sizeof(NeteaseMultiband);}
size_t netease_multiband_alignment(void){return _Alignof(NeteaseMultiband);}
int netease_multiband_set(NeteaseMultiband *s,const unsigned char *raw,size_t bytes) {
    if(!s || !s->initialized)return -1;
    NeteaseDynamicsConfig c;if(dynamics_decode(&c,raw,bytes))return -1;
    /* C6F40 fixes the crossover order during Init, even if the parameter blob
     * subsequently changes its flag. C3840 only updates coefficients. */
    s->config=c;
    for(unsigned i=0;i<2;i++)crossover_configure(&s->crossover[i],s->rate,c.frequency);
    return 0;
}
int netease_multiband_reset(NeteaseMultiband *s) {
    if(!s || !s->initialized)return -1;
    memset(s->detector,0,sizeof(s->detector));
    for(unsigned stream=0;stream<2;stream++)for(unsigned i=0;i<22;i++)
        netease_biquad_reset(&s->crossover[stream].filters[i]);
    return 0;
}
int netease_multiband_init(NeteaseMultiband *s,float rate,unsigned capacity,const unsigned char *raw,size_t bytes) {
    NeteaseDynamicsConfig c;
    if(!s || !isfinite(rate) || rate<8000 || rate>384000 || !capacity || capacity>4096 ||
       dynamics_decode(&c,raw,bytes))return -1;
    memset(s,0,sizeof(*s));s->rate=rate;s->capacity=capacity;s->initialized=1;
    for(unsigned i=0;i<2;i++)s->crossover[i].fourth=c.fourth;
    netease_multiband_set(s,raw,bytes);s->rms=expf(-43.066322326660156f/rate);
    s->input=c.input;s->output=c.output;
    for(unsigned i=0;i<5;i++){s->band_input[i]=c.band[i].input;s->band_output[i]=c.band[i].output;}
    return 0;
}
int netease_multiband_process(NeteaseMultiband *s,float *main,float *side,size_t n) {
    if(!s || !s->initialized || !main || main==side || n>s->capacity)return -1;
    if(!n)return 0;
    unsigned csr=begin_plugin();NeteaseDynamicsConfig *c=&s->config;
    netease_peq_gain(main,n,s->input,c->input);
    if(side)netease_peq_gain(side,n,s->input,c->input);
    s->input=c->input;
    memcpy(s->data[0][0],main,n*4);memset(main,0,s->capacity*4);
    crossover_process(&s->crossover[0],s->data[0],n,c->count);
    if(side){memcpy(s->data[1][0],side,n*4);crossover_process(&s->crossover[1],s->data[1],n,c->count);}
    for(unsigned band=0;band<c->count;band++) {
        float *data=s->data[0][band];NeteaseDynamicsBand p=c->band[band];
        netease_peq_gain(data,n,s->band_input[band],p.input);s->band_input[band]=p.input;
        detector_process(&s->detector[band],data,side?s->data[1][band]:NULL,n,s->rate,s->rms,p);
        netease_mix(main,data,n,1,1,s->band_output[band],p.output);s->band_output[band]=p.output;
    }
    netease_peq_gain(main,n,s->output,c->output);s->output=c->output;
    end_plugin(csr);return 0;
}
int netease_multiband_history(const NeteaseMultiband *s,float out[10]) {
    if(!s || !s->initialized || !out)return -1;
    for(unsigned i=0;i<5;i++){out[2*i]=s->detector[i].envelope;out[2*i+1]=s->detector[i].power;}
    return 0;
}
