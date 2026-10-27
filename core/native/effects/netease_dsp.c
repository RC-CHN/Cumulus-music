/* Portable DSP fragments recovered from neaudioeffects.dll 3.1.42.205594 x64.
 * Coefficient equations: RVA B54D0. Matrix: A7750. Processing: B4DB0/B8B10.
 * This is a filter kernel, not the complete official effects engine.
 * Compile with -ffp-contract=off, without -ffast-math; see the validation report.
 */
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <stddef.h>
/* Older libm tanf implementations differ by one float ULP at crossover
 * inputs. Evaluate the same float argument in double, then round once to
 * float. This preserves all frozen native vectors on glibc 2.39 and 2.43. */
static float filter_tangent(float value) { return (float)tan((double)value); }
#if defined(__SSE__) || defined(_M_X64) || (defined(_M_IX86_FP) && _M_IX86_FP >= 1)
#include <xmmintrin.h>
#endif

typedef struct {
    float matrix[8][4];
    float b0,b1,b2,f1,f2,padding[3];
    float x1,x2,y1,y2;
} NeteaseBiquad;
_Static_assert(sizeof(NeteaseBiquad)==176,"filter layout changed");

/* Inputs use additive feedback: y=...+f1*y[-1]+f2*y[-2]. Preserve history. */
void netease_biquad_set(NeteaseBiquad *s,const float c[5]) {
    float b0=c[0],b1=c[1],b2=c[2],f=c[3],g=c[4];
    float t=b0*f+b1;
    float u=(t*f+b2)+b0*g;
    float v=(u*f+(f*g)*b0)+b1*g;
    float d=b1*f+b2;
    float e=d*f+b1*g;
    float f_squared=f*f, g_squared=g*g;
    float matrix[8][4]={
        {b0,b0,b0,b0},
        {0,0,0,t},
        {0,0,t,u},
        {0,t,u,v},
        {b1,d,e,(e+b1*g)*f+b2*g},
        {b2,b2*f,(b2*f)*f+b2*g,((b2+b2)*g+(b2*f)*f)*f},
        {f,f_squared+g,((g+g)+f_squared)*f,((g*3.0f+f_squared)*f)*f+g_squared},
        {g,f*g,(f*g)*f+g_squared,((g+g)*g+(f*g)*f)*f}
    };
    memcpy(s->matrix,matrix,sizeof(matrix));
    s->b0=b0;s->b1=b1;s->b2=b2;s->f1=f;s->f2=g;
}

void netease_biquad_reset(NeteaseBiquad *s) {
    s->x1=s->x2=s->y1=s->y2=0;
}

static void scalar(NeteaseBiquad *s,float *data,size_t n) {
    for(size_t i=0;i<n;i++) {
        float x=data[i];
        float y=(((x*s->b0+s->x2*s->b2)+s->x1*s->b1)+s->y2*s->f2)+s->y1*s->f1;
        s->x2=s->x1;s->x1=x;s->y2=s->y1;s->y1=y;data[i]=y;
    }
}

/* Match the official four-sample expansion and scalar alignment/tail paths. */
void netease_biquad_process(NeteaseBiquad *s,float *data,size_t n) {
    size_t misalignment=(uintptr_t)data&15;
    size_t prefix=misalignment?(16-misalignment)/4:0;
    if(prefix>n)prefix=n;
    scalar(s,data,prefix);data+=prefix;n-=prefix;
    size_t count=n&~(size_t)3;
    for(size_t i=0;i<count;i+=4) {
        float x[4],y[4];memcpy(x,data+i,sizeof(x));
        for(int k=0;k<4;k++) {
            float a=s->x2*s->matrix[5][k]+s->x1*s->matrix[4][k];
            float b=s->y2*s->matrix[7][k]+s->y1*s->matrix[6][k];
            b=b+x[2]*s->matrix[1][k];
            float c=x[k]*s->matrix[0][k]+a;
            float d=x[1]*s->matrix[2][k]+x[0]*s->matrix[3][k];
            y[k]=(b+c)+d;
        }
        memcpy(data+i,y,sizeof(y));s->x1=x[3];s->x2=x[2];s->y1=y[3];s->y2=y[2];
    }
    scalar(s,data+count,n-count);
}

static float from_bits(uint32_t bits) {float f;memcpy(&f,&bits,4);return f;}
static uint32_t float_bits(float f) {uint32_t bits;memcpy(&bits,&f,4);return bits;}

/* Scalar translation of the SSE sine fast path at RVA 134110. The filter's
 * frequency clamp keeps both phases below 4.4 radians, within this path.
 * The complete math library's >10000-radian reduction is outside this kernel.
 */
static float filter_sine(float x) {
    float absolute=fabsf(x);
    float rounded=absolute*0.31830987334251404f+12582912.0f;
    float n=rounded-12582912.0f;
    float r=absolute-n*3.140625f;
    r=r-n*0.0009675025939941406f;
    r=r-n*1.5099067240953445e-7f;
    r=r-n*5.126688136514179e-12f;
    float squared=r*r;
    r=from_bits(float_bits(r)^((float_bits(rounded)&1u)<<31));
    float p=2.605780537123792e-6f*squared-0.00019809602235909551f;
    p=p*squared+0.008333062753081322f;
    p=p*squared-0.16666659712791443f;
    float answer=r+(squared*p)*r;
    return from_bits(float_bits(answer)^(float_bits(x)&0x80000000u));
}

/* Official fast 10^x approximation; gain/40 is the input to this function. */
static float fast_power10(float x) {
    if(x < -37.0f)return 0;
    float encoded_float=x*27866352.0f+1065353216.0f;
    /* Defined failure for pathological dynamics amplification. Normal GEQ
     * inputs retain the same float rounding and integer conversion. */
    if(!isfinite(encoded_float) || encoded_float < -9223372036854775808.0f ||
       encoded_float >= 9223372036854775808.0f)return NAN;
    int64_t encoded=(int64_t)encoded_float;
    float m=from_bits(((uint32_t)encoded&0x7fffff)+0x3f800000);
    float exponent=from_bits((uint32_t)encoded&0xff800000);
    return ((m*0.3251897692680359f+0.02080577239394188f)*m+0.6530434489250183f)*exponent;
}

/* Internal DSP enum, NOT the JSON PEQ type enum:
 * 0 low shelf, 1 peak, 2 high shelf, 3 low pass, 4 high pass, 5 band pass,
 * 6 notch, 7 low shelf with Q, 8 high shelf with Q.
 * Portable libm trig is numerically compared, not claimed bit-identical to CRT.
 */
static int coefficients(unsigned type,float rate,float frequency,float gain,float q,float out[5],int peq) {
    if(type>10 || !isfinite(rate) || !isfinite(frequency) || !isfinite(gain) || !isfinite(q)
       || rate<=0 || frequency<=0 || q<=0 || gain < -120 || gain > 120 || !out)return -1;
    float f=fminf(rate*0.44999998807907104f,frequency);
    float b0=0,b1=0,b2=0,a0=0,a1=0,a2=0;
    if(type==3 || type==4) {
        float t=filter_tangent((f*3.1415927410125732f)/rate);
        if(type==3)t=1.0f/t;
        float squared=t*t,term=t*1.4142135381698608f;
        float inv=1.0f/((squared+1.0f)+term);
        b0=b2=inv;b1=type==3?inv+inv:inv*(-2.0f);
        a1=(1.0f-squared)*b1;a2=((squared+1.0f)-term)*inv;a0=1.0f;
    } else {
        float w=(f*6.2831854820251465f)/rate;
        float sine,cosine;
        if(type==1 && !peq) {sine=sinf(w);cosine=cosf(w);}
        else if(w<0.0001220703125f) {sine=w;cosine=1.0f-(w*w)*0.5f;}
        else {
            /* RVA 133340 computes a pair of sines; rounding the phase offset
             * is observable at low frequencies, so cosf(w) is not equivalent.
             */
            sine=filter_sine(w);cosine=filter_sine(w+1.5707963705062866f);
        }
        float alpha=sine*(0.5f/q);
        if(type==9 || type==10) {
            b0=b2=(type==9?1.0f-cosine:cosine+1.0f)*0.5f;
            b1=type==9?b0+b0:b0*(-2.0f);
            a0=alpha+1.0f;a1=cosine*(-2.0f);a2=1.0f-alpha;
        } else if(type==5 || type==6) {
            b0=type==5?alpha:1.0f;b1=type==5?0:cosine*(-2.0f);b2=type==5?-alpha:1.0f;
            a0=alpha+1.0f;a1=cosine*(-2.0f);a2=1.0f-alpha;
        } else {
            float x=gain*0.02500000037252903f;
            float A=peq?powf(10.0f,x):fast_power10(x);
            if(type==1) {
                float product=alpha*A,quotient=alpha/A;
                b0=product+1.0f;b1=cosine*(-2.0f);b2=1.0f-product;
                a0=quotient+1.0f;a1=b1;a2=1.0f-quotient;
            } else {
                float u=A+1.0f,v=A-1.0f;
                float k=(type==0 || type==2)?(sine*sqrtf(A))*1.4142135381698608f:alpha*(sqrtf(A)+sqrtf(A));
                if(type==0 || type==7) {
                    float vc=v*cosine,uc=u*cosine;
                    b0=((u-vc)+k)*A;b1=(A+A)*(v-uc);b2=((u-vc)-k)*A;
                    a0=(u+vc)+k;a1=v*(-2.0f)-(uc+uc);a2=(u+vc)-k;
                } else {
                    float vc=v*cosine,uc=u*cosine;
                    b0=((vc+u)+k)*A;b1=((uc+v)*A)*(-2.0f);b2=((vc+u)-k)*A;
                    a0=(u-vc)+k;a1=(v-uc)+(v-uc);a2=(u-vc)-k;
                }
            }
        }
    }
    float inv=1.0f/a0;
    float result[5]={inv*b0,inv*b1,inv*b2,-(inv*a1),-(inv*a2)};
    for(int i=0;i<5;i++)if(!isfinite(result[i]))return -2;
    memcpy(out,result,sizeof(result));return 0;
}

int netease_biquad_coefficients(unsigned type,float rate,float frequency,float gain,float q,float out[5]) {
    if(type>8)return -1;
    return coefficients(type,rate,frequency,gain,q,out,0);
}

/* The separate parameter-EQ plugin uses powf and paired sine even for peaks.
 * Its 11-value enum differs from the GEQ coefficient helper above. This is
 * the native plugin enum, not a claim about every JSON material's translation.
 */
int netease_peq_coefficients(unsigned type,float rate,float frequency,float gain,float q,float out[5]) {
    static const unsigned mapping[11]={3,4,5,6,0,2,1,7,8,9,10};
    if(type>10)return -1;
    /* Official materials include q=0 on Butterworth filters. These four
     * branches never read Q; permit finite unused values without dividing. */
    if(isfinite(q) && (type==0 || type==1 || type==4 || type==5))q=1;
    return coefficients(mapping[type],rate,frequency,gain,q,out,1);
}

/* Default ten-band GEQ core. Native plugin uses shelf endpoints by default.
 * Does not include the outer activation crossfade or custom plugin settings.
 */
typedef struct {float rate;unsigned channels;NeteaseBiquad filters[8][10];} NeteaseGeq;

int netease_geq_set(NeteaseGeq *s,const float gains[10]) {
    if(!s || !gains || !s->channels || s->channels>8)return -1;
    float coefficients[10][5];
    float edge=sqrtf(31.25f*62.5f);
    for(unsigned band=0;band<10;band++) {
        unsigned type=band==0?7:band==9?8:1;
        float frequency=band==0?edge:band==9?edge*256.0f:31.25f*(float)(1u<<band);
        float q=band==0 || band==9?0.707f:sqrtf(2.0f);
        int result=netease_biquad_coefficients(type,s->rate,frequency,gains[band],q,coefficients[band]);
        if(result)return result;
    }
    for(unsigned ch=0;ch<s->channels;ch++)for(unsigned band=0;band<10;band++)
        netease_biquad_set(&s->filters[ch][band],coefficients[band]);
    return 0;
}

int netease_geq_init(NeteaseGeq *s,float rate,unsigned channels) {
    if(!s || !isfinite(rate) || rate<=0 || !channels || channels>8)return -1;
    memset(s,0,sizeof(*s));s->rate=rate;s->channels=channels;
    const float zero[10]={0};return netease_geq_set(s,zero);
}

void netease_geq_process(NeteaseGeq *s,float *planar,size_t frames,size_t channel_stride) {
    if(!s || !planar || frames>channel_stride || !s->channels || s->channels>8)return;
    for(unsigned ch=0;ch<s->channels;ch++)for(unsigned band=0;band<10;band++)
        netease_biquad_process(&s->filters[ch][band],planar+ch*channel_stride,frames);
}

/* RVA A9C50. The ramp uses i/N, so its final sample is short of the endpoint.
 * Preserve repeated float additions rather than replacing them by i*step.
 * Unlike the native equal-gain SIMD fast path, never touch beyond n samples.
 */
void netease_mix(float *first,const float *second,size_t n,float first_start,float first_end,
                 float second_start,float second_end) {
    if(!first || !second || !n)return;
    if(first_start==first_end && second_start==second_end) {
        for(size_t i=0;i<n;i++)first[i]=second[i]*second_start+first[i]*first_start;
        return;
    }
    float inverse=1.0f/(float)n;
    float da=(first_end-first_start)*inverse,db=(second_end-second_start)*inverse;
    for(size_t i=0;i<n;i++) {
        first[i]=first[i]*first_start+second[i]*second_start;
        first_start=first_start+da;second_start=second_start+db;
    }
}

typedef struct {NeteaseGeq core;unsigned capacity,bypass,previous;} NeteaseGeqStage;
size_t netease_geq_stage_size(void) {return sizeof(NeteaseGeqStage);}

int netease_geq_stage_init(NeteaseGeqStage *s,float rate,unsigned channels,unsigned capacity) {
    if(!s || !capacity || capacity>4096)return -1;
    int status=netease_geq_init(&s->core,rate,channels);
    if(status)return status;
    s->capacity=capacity;s->bypass=s->previous=0;return 0;
}
int netease_geq_stage_set(NeteaseGeqStage *s,const float gains[10]) {
    return s?netease_geq_set(&s->core,gains):-1;
}
int netease_geq_stage_bypass(NeteaseGeqStage *s,unsigned bypass) {
    if(!s || bypass>1)return -1;
    if(!bypass && s->bypass)
        for(unsigned ch=0;ch<s->core.channels;ch++)for(unsigned b=0;b<10;b++)
            netease_biquad_reset(&s->core.filters[ch][b]);
    s->bypass=bypass;return 0;
}

static void blend_channels(float *planar,const float *dry,unsigned channels,unsigned capacity,unsigned bypass) {
    for(unsigned ch=0;ch<channels;ch++)
        netease_mix(planar+ch*capacity,dry+ch*capacity,capacity,
                    bypass?1.0f:0.0f,bypass?0.0f:1.0f,bypass?0.0f:1.0f,bypass?1.0f:0.0f);
}
/* A1BA0 enables SSE flush-to-zero only during plugin execution and restores
 * the caller's FTZ bit before blending. Other architectures retain their
 * host floating-point environment; only Linux x86_64 has been compared here.
 */
static unsigned begin_plugin(void) {
#if defined(__SSE__) || defined(_M_X64) || (defined(_M_IX86_FP) && _M_IX86_FP >= 1)
    unsigned old=_mm_getcsr();_mm_setcsr(old|0x8000u);return old&0x8000u;
#else
    return 0;
#endif
}
static void end_plugin(unsigned old) {
#if defined(__SSE__) || defined(_M_X64) || (defined(_M_IX86_FP) && _M_IX86_FP >= 1)
    _mm_setcsr((_mm_getcsr()&~0x8000u)|old);
#else
    (void)old;
#endif
}
int netease_geq_stage_process(NeteaseGeqStage *s,float *planar,size_t frames,float *dry) {
    if(!s || !planar || frames>s->capacity || !s->capacity || !s->core.channels || s->core.channels>8)return -1;
    unsigned changing=s->bypass!=s->previous;
    if(changing && (!dry || dry==planar))return -1;
    if(changing)memcpy(dry,planar,s->core.channels*s->capacity*sizeof(float));
    if(changing || !s->bypass) {
        unsigned old=begin_plugin();
        netease_geq_process(&s->core,planar,frames,s->capacity);
        end_plugin(old);
    }
    if(changing)blend_channels(planar,dry,s->core.channels,s->capacity,s->bypass);
    s->previous=s->bypass;return 0;
}

/* BassTrebleProcessor: native defaults are three enabled filters, two fixed
 * shelves (120Hz/5kHz) and a neutral 2kHz peak. Q=5 for all three; Q is ignored
 * by the fixed shelves. The other six slots of its nine-slot plugin are off.
 */
typedef struct {
    float rate;unsigned channels,capacity,bypass,previous;
    NeteaseBiquad filters[8][3];
} NeteaseBassTreble;
size_t netease_bt_size(void) {return sizeof(NeteaseBassTreble);}

int netease_bt_set(NeteaseBassTreble *s,float bass,float treble,float low_frequency,float low_q,
                    float high_frequency,float high_q) {
    if(!s || !s->channels || s->channels>8 || !isfinite(bass) || !isfinite(treble))return -1;
    /* The parameter setter clamps gain at +/-24dB before marking it dirty. */
    bass=fminf(24,fmaxf(-24,bass));treble=fminf(24,fmaxf(-24,treble));
    float c[3][5];
    int status=netease_peq_coefficients(4,s->rate,low_frequency,bass,low_q,c[0]);
    if(!status)status=netease_peq_coefficients(6,s->rate,2000,0,5,c[1]);
    if(!status)status=netease_peq_coefficients(5,s->rate,high_frequency,treble,high_q,c[2]);
    if(status)return status;
    for(unsigned ch=0;ch<s->channels;ch++)for(unsigned b=0;b<3;b++)netease_biquad_set(&s->filters[ch][b],c[b]);
    return 0;
}
int netease_bt_init(NeteaseBassTreble *s,float rate,unsigned channels,unsigned capacity) {
    if(!s || !isfinite(rate) || rate<=0 || !channels || channels>8 || !capacity || capacity>4096)return -1;
    memset(s,0,sizeof(*s));s->rate=rate;s->channels=channels;s->capacity=capacity;
    s->bypass=s->previous=0;return netease_bt_set(s,0,0,120,5,5000,5);
}
int netease_bt_bypass(NeteaseBassTreble *s,unsigned bypass) {
    if(!s || bypass>1)return -1;
    if(!bypass && s->bypass)
        for(unsigned ch=0;ch<s->channels;ch++)for(unsigned b=0;b<3;b++)netease_biquad_reset(&s->filters[ch][b]);
    s->bypass=bypass;return 0;
}
int netease_bt_process(NeteaseBassTreble *s,float *planar,size_t frames,float *dry) {
    if(!s || !planar || frames>s->capacity || !s->capacity || !s->channels || s->channels>8)return -1;
    unsigned changing=s->bypass!=s->previous;
    if(changing && (!dry || dry==planar))return -1;
    if(changing)memcpy(dry,planar,s->channels*s->capacity*sizeof(float));
    if(changing || !s->bypass) {
        unsigned old=begin_plugin();
        for(unsigned ch=0;ch<s->channels;ch++)for(unsigned b=0;b<3;b++)
            netease_biquad_process(&s->filters[ch][b],planar+ch*s->capacity,frames);
        end_plugin(old);
    }
    if(changing)blend_channels(planar,dry,s->channels,s->capacity,s->bypass);
    s->previous=s->bypass;return 0;
}

/* Parameter-EQ plugin's output ramp, RVA AC650. The SIMD prefix distributes
 * over floor(n/4)*4. A 1..3-sample tail starts from the old gain again. This is
 * observable native behavior, not a conventional uniform linear ramp.
 */
void netease_peq_gain(float *data,size_t n,float old,float target) {
    if(!data || !n)return;
    if(old==target) {
        if(old!=1)for(size_t i=0;i<n;i++)data[i]=data[i]*old;
        return;
    }
    size_t vector=n&~(size_t)3;
    if(vector) {
        float delta=(target-old)/(float)vector;
        float four=delta*4.0f;
        float gains[4];gains[0]=old;gains[1]=old+delta;
        gains[2]=gains[1]+delta;gains[3]=gains[2]+delta;
        for(size_t i=0;i<vector;i+=4)for(unsigned k=0;k<4;k++) {
            data[i+k]=data[i+k]*gains[k];gains[k]=gains[k]+four;
        }
    }
    float gain=old,delta=(target-old)/(float)n;
    for(size_t i=vector;i<n;i++){data[i]=data[i]*gain;gain=gain+delta;}
}

typedef struct {unsigned type;float gain,frequency,q;unsigned enabled;} NeteasePeqBand;
_Static_assert(sizeof(NeteasePeqBand)==20,"parameter layout changed");
typedef struct {
    float rate;unsigned channels,capacity,has_lfe,filter_channels,process_lfe,bypass,previous;
    float gain_db,current_gain;
    NeteasePeqBand bands[9];unsigned dirty[9];NeteaseBiquad filters[8][9];
} NeteasePeq;
size_t netease_peq_size(void) {return sizeof(NeteasePeq);}

int netease_peq_band(NeteasePeq *s,unsigned band,unsigned type,float gain,float frequency,float q,unsigned enabled) {
    if(!s || !s->channels || s->channels>8 || band>8 || enabled>1 || !isfinite(gain))return -1;
    gain=fminf(24,fmaxf(-24,gain));
    float c[5];int status=netease_peq_coefficients(type,s->rate,frequency,gain,q,c);
    if(status)return status;
    NeteasePeqBand next={type,gain,frequency,q,enabled};
    if(memcmp(&next,&s->bands[band],sizeof(next))){s->bands[band]=next;s->dirty[band]=1;}
    return 0;
}
int netease_peq_set_gain(NeteasePeq *s,float gain) {
    if(!s || !isfinite(gain))return -1;
    s->gain_db=fminf(24,fmaxf(-24,gain));return 0;
}
int netease_peq_set_lfe(NeteasePeq *s,unsigned process_lfe) {
    if(!s || process_lfe>1)return -1;
    /* Filter channel count is fixed by native Init; only gain routing reads
     * this parameter again during Execute. Preserve that distinction. */
    s->process_lfe=process_lfe;return 0;
}
int netease_peq_init(NeteasePeq *s,float rate,unsigned channels,unsigned capacity,unsigned has_lfe,unsigned process_lfe) {
    if(!s || !isfinite(rate) || rate<8000 || rate>384000 || !channels || channels>8 ||
       !capacity || capacity>4096 || has_lfe>1 || process_lfe>1)return -1;
    memset(s,0,sizeof(*s));s->rate=rate;s->channels=channels;s->capacity=capacity;
    s->has_lfe=has_lfe;s->process_lfe=process_lfe;
    s->filter_channels=channels-(has_lfe && !process_lfe);s->current_gain=1;
    for(unsigned b=0;b<9;b++) {
        unsigned type=b==0?4:b==1?6:5;
        float frequency=b==0?120.0f:b==1?2000.0f:5000.0f;
        int status=netease_peq_band(s,b,type,0,frequency,5,b<3);
        if(status)return status;
    }
    return 0;
}
int netease_peq_bypass(NeteasePeq *s,unsigned bypass) {
    if(!s || bypass>1)return -1;
    if(!bypass && s->bypass)
        for(unsigned ch=0;ch<s->channels;ch++)for(unsigned b=0;b<9;b++)netease_biquad_reset(&s->filters[ch][b]);
    s->bypass=bypass;return 0;
}
int netease_peq_process(NeteasePeq *s,float *planar,size_t frames,float *dry) {
    if(!s || !planar || !s->capacity || frames>s->capacity || !s->channels || s->channels>8)return -1;
    unsigned changing=s->bypass!=s->previous;
    if(changing && (!dry || dry==planar))return -1;
    if(changing)memcpy(dry,planar,s->channels*s->capacity*sizeof(float));
    if((changing || !s->bypass) && frames && s->filter_channels) {
        unsigned old=begin_plugin();
        for(unsigned b=0;b<9;b++)if(s->dirty[b]) {
            NeteasePeqBand p=s->bands[b];float c[5];
            /* Public setters already validated the exact parameters. */
            netease_peq_coefficients(p.type,s->rate,p.frequency,p.gain,p.q,c);
            for(unsigned ch=0;ch<s->filter_channels;ch++)netease_biquad_set(&s->filters[ch][b],c);
            s->dirty[b]=0;
        }
        for(unsigned ch=0;ch<s->filter_channels;ch++)for(unsigned b=0;b<9;b++)
            if(s->bands[b].enabled)netease_biquad_process(&s->filters[ch][b],planar+ch*s->capacity,frames);
        float target=powf(10.0f,s->gain_db*0.05000000074505806f);
        unsigned gain_channels=s->channels-(s->has_lfe && !s->process_lfe);
        for(unsigned ch=0;ch<gain_channels;ch++)netease_peq_gain(planar+ch*s->capacity,frames,s->current_gain,target);
        s->current_gain=target;end_plugin(old);
    }
    if(changing)blend_channels(planar,dry,s->channels,s->capacity,s->bypass);
    s->previous=s->bypass;return 0;
}
