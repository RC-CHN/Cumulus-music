/* Rotate3D 44.1 kHz / 256-frame kernel recovered from neaudioeffects.dll.
 * Official response interpolation/convolution/state order, independent radix-2
 * FFT and the 44.1 kHz stereo FIFO wrapper. External rate conversion is pending.
 * -std=c11 -O2 -ffp-contract=off -fno-fast-math -fPIC -shared ... -lm
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#define ROTATE_TABLE_FLOATS (2*37*1026)
typedef struct {
    const float *tables;
    float phase,period,step;
    unsigned warm;
    float overlap[4][256];
    float response[2][1026];
    float spectrum[6][2048];
} NeteaseRotate;

/* Interleaved complex transform; both directions are unnormalized. */
int netease_rotate_fft(float *data,unsigned inverse) {
    if(!data || inverse>1)return -1;
    for(unsigned i=1,j=0;i<1024;i++) {
        unsigned bit=512;
        for(;j&bit;bit>>=1)j^=bit;
        j^=bit;
        if(i<j)for(unsigned k=0;k<2;k++) {
            float x=data[2*i+k];data[2*i+k]=data[2*j+k];data[2*j+k]=x;
        }
    }
    for(unsigned length=2;length<=1024;length*=2) {
        for(unsigned k=0;k<length/2;k++) {
            double angle=(inverse?1.0:-1.0)*6.2831853071795864769*k/length;
            float wr=(float)cos(angle),wi=(float)sin(angle);
            for(unsigned i=k;i<1024;i+=length) {
                unsigned j=i+length/2;
                float tr=wr*data[2*j]-wi*data[2*j+1];
                float ti=wi*data[2*j]+wr*data[2*j+1];
                float ar=data[2*i],ai=data[2*i+1];
                data[2*i]=ar+tr;data[2*i+1]=ai+ti;
                data[2*j]=ar-tr;data[2*j+1]=ai-ti;
            }
        }
    }
    return 0;
}

size_t netease_rotate_size(void){return sizeof(NeteaseRotate);}
int netease_rotate_period(NeteaseRotate *s,float seconds) {
    if(!s || !isfinite(seconds))return -1;
    if(seconds<1)return 1; /* Official outer setter ignores values below 1. */
    s->period=seconds;s->step=(float)(((256.0/44100.0)*360.0)/(double)seconds);
    return 0;
}
int netease_rotate_init(NeteaseRotate *s,const float *tables,size_t count,float phase) {
    if(!s || !tables || count!=ROTATE_TABLE_FLOATS || !isfinite(phase) || phase<0 || phase>=360)return -1;
    for(size_t i=0;i<count;i++)if(!isfinite(tables[i]))return -2;
    memset(s,0,sizeof(*s));s->tables=tables;s->phase=phase;
    return netease_rotate_period(s,40);
}
void netease_rotate_reset(NeteaseRotate *s) {
    memset(s->overlap,0,sizeof(s->overlap));s->warm=0;
}
float netease_rotate_phase(const NeteaseRotate *s){return s->phase;}
const float *netease_rotate_response(const NeteaseRotate *s){return s->response[0];}

static void response(NeteaseRotate *s) {
    float position=s->phase/5.0f;
    unsigned low=(unsigned)floor((double)position),high=low+1;
    float a=(float)high-position,b=1.0f-a;
    unsigned mirror=s->phase>=180;
    if(mirror){low=72-low;high=72-high;}
    for(unsigned ear=0;ear<2;ear++) {
        const float *table=s->tables+(ear^mirror)*37*1026;
        for(unsigned i=0;i<1026;i++)s->response[ear][i]=b*table[high*1026+i]+a*table[low*1026+i];
    }
}

/* Full planar storage is 2x256 floats; valid_frames is updated after warm-up.
 * Input past valid_frames remains unchanged, but state advances as the original
 * kernel does. The outer wrapper normally does not invoke this with zero input.
 */
int netease_rotate_process(NeteaseRotate *s,float *planar,unsigned *valid_frames) {
    if(!s || !s->tables || !planar || !valid_frames || *valid_frames>256)return -1;
    unsigned n=*valid_frames;
    memset(s->spectrum[0],0,2*sizeof(s->spectrum[0]));
    for(unsigned i=0;i<n;i++) {
        float sum=planar[i]+planar[256+i],difference=planar[i]-planar[256+i];
        s->spectrum[0][2*i]=(float)((double)sum*0.707);
        s->spectrum[1][2*i]=(float)((double)difference*0.707);
    }
    netease_rotate_fft(s->spectrum[0],0);netease_rotate_fft(s->spectrum[1],0);
    response(s);
    const float *side=s->tables+37*1026+18*1026;
    for(unsigned k=0;k<513;k++) {
        unsigned j=2*k;
        float mr=s->spectrum[0][j],mi=s->spectrum[0][j+1];
        float sr=s->spectrum[1][j],si=s->spectrum[1][j+1];
        for(unsigned ear=0;ear<2;ear++) {
            float hr=s->response[ear][j],hi=s->response[ear][j+1];
            s->spectrum[2+ear][j]=hr*mr-mi*hi;
            s->spectrum[2+ear][j+1]=mi*hr+mr*hi;
        }
        float hr=side[j],hi=side[j+1];
        s->spectrum[4][j]=hr*sr-hi*si;
        s->spectrum[4][j+1]=hr*si+hi*sr;
        s->spectrum[5][j]=hi*si-hr*sr;
        s->spectrum[5][j+1]=(-sr)*hi-hr*si;
    }
    for(unsigned channel=0;channel<4;channel++) {
        float *out=s->spectrum[channel+2];
        for(unsigned k=513;k<1024;k++){out[2*k]=out[2*(1024-k)];out[2*k+1]=-out[2*(1024-k)+1];}
        netease_rotate_fft(out,1);
    }
    for(unsigned i=0;i<n;i++) {
        float left=s->spectrum[4][2*i]*(1.0f/1024)+s->overlap[2][i];
        float mid=s->spectrum[2][2*i]*(1.0f/1024)+s->overlap[0][i];
        planar[i]=left+mid;
        float right=s->spectrum[5][2*i]*(1.0f/1024)+s->overlap[3][i];
        mid=s->spectrum[3][2*i]*(1.0f/1024)+s->overlap[1][i];
        planar[256+i]=right+mid;
    }
    for(unsigned ch=0;ch<4;ch++)for(unsigned i=0;i<256;i++)
        s->overlap[ch][i]=s->spectrum[ch+2][2*(256+i)]*(1.0f/1024);
    float phase=s->phase+s->step;
    s->phase=(float)fmod((double)phase,360.0);
    if(!s->warm){s->warm=1;*valid_frames=0;}
    return 0;
}

/* Rotate3DProcessorImpl 22EA0/23140, for its native stereo 44.1 kHz path.
 * Capacities below 256 cannot hold the official flush block and are rejected.
 */
#define ROTATE_FIFO_CAPACITY (2*4096+512)
typedef struct {unsigned count;float pcm[2][ROTATE_FIFO_CAPACITY];} RotateFifo;
typedef struct {
    NeteaseRotate kernel;
    unsigned capacity,bypass,previous;
    float period;
    RotateFifo input,output;
} NeteaseRotateStage;

static void fifo_append(RotateFifo *q,const float *planar,unsigned stride,unsigned count) {
    for(unsigned ch=0;ch<2;ch++)memcpy(q->pcm[ch]+q->count,planar+ch*stride,count*sizeof(float));
    q->count+=count;
}
static unsigned fifo_read(RotateFifo *q,float *planar,unsigned stride,unsigned requested) {
    unsigned n=q->count<requested?q->count:requested;
    for(unsigned ch=0;ch<2;ch++) {
        memcpy(planar+ch*stride,q->pcm[ch],n*sizeof(float));
        memmove(q->pcm[ch],q->pcm[ch]+n,(q->count-n)*sizeof(float));
    }
    q->count-=n;return n;
}
static void stage_flush(NeteaseRotateStage *s) {
    float block[512]={0};unsigned n=256;
    netease_rotate_process(&s->kernel,block,&n);
    if(n)fifo_append(&s->output,block,256,n);
    netease_rotate_reset(&s->kernel);s->kernel.phase=0;
    while(s->input.count) {
        n=fifo_read(&s->input,block,256,256);
        fifo_append(&s->output,block,256,n);
    }
}
size_t netease_rotate_stage_size(void){return sizeof(NeteaseRotateStage);}
int netease_rotate_stage_init(NeteaseRotateStage *s,const float *tables,size_t count,unsigned capacity) {
    if(!s || capacity<256 || capacity>4096)return -1;
    /* Validate the table before modifying the caller's stage. */
    if(!tables || count!=ROTATE_TABLE_FLOATS)return -1;
    for(size_t i=0;i<count;i++)if(!isfinite(tables[i]))return -2;
    memset(s,0,sizeof(*s));s->capacity=capacity;s->bypass=s->previous=1;s->period=40;
    return netease_rotate_init(&s->kernel,tables,count,0);
}
int netease_rotate_stage_period(NeteaseRotateStage *s,float seconds) {
    if(!s || !isfinite(seconds))return -1;
    if(seconds<1)return 1;
    s->period=seconds;return 0;
}
int netease_rotate_stage_bypass(NeteaseRotateStage *s,unsigned bypass) {
    if(!s || bypass>1)return -1;
    s->bypass=bypass;return 0;
}
void netease_rotate_stage_reset(NeteaseRotateStage *s) {
    netease_rotate_reset(&s->kernel);s->input.count=s->output.count=0;
}
float netease_rotate_stage_phase(const NeteaseRotateStage *s){return s->kernel.phase;}
int netease_rotate_stage_process(NeteaseRotateStage *s,float *planar,unsigned *valid_frames,unsigned *end) {
    if(!s || !s->kernel.tables || !planar || !valid_frames || !end ||
       *valid_frames>s->capacity || *end>1)return -1;
    unsigned n=*valid_frames;
    /* Bound all temporary occupancy before any mutation. Normal callback use
     * never reaches this guard; it also protects unusual repeated short blocks. */
    if(s->input.count+n>ROTATE_FIFO_CAPACITY ||
       s->output.count+s->input.count+n+256>ROTATE_FIFO_CAPACITY)return -2;
    if(s->bypass) {
        if(s->previous!=s->bypass){s->previous=s->bypass;stage_flush(s);}
        if(!s->output.count)return 0;
        if(n)fifo_append(&s->output,planar,s->capacity,n);
    } else {
        s->previous=s->bypass;
        if(s->kernel.period!=s->period)netease_rotate_period(&s->kernel,s->period);
        if(n)fifo_append(&s->input,planar,s->capacity,n);
        while(s->input.count>=256 || *end) {
            float block[512]={0};unsigned count=fifo_read(&s->input,block,256,256);
            if(!count){if(*end)stage_flush(s);break;}
            netease_rotate_process(&s->kernel,block,&count);
            if(count)fifo_append(&s->output,block,256,count);
        }
    }
    unsigned available=s->output.count;
    unsigned count=(*end || available>=s->capacity)?available:0;
    if(count>s->capacity)count=s->capacity;
    *valid_frames=fifo_read(&s->output,planar,s->capacity,count);
    if(*end)*end=!s->output.count;
    return 0;
}
