/* Official PC3.1.42 x64 fixed SE bank1 environment child: B6D10/B72B0,
 * B6EB0/BA4C0/BE020. No ER, arbitrary bank, complete SE or live UI claim.
 * A Rust-owned zeroed arena backs every delay. Init uses CRT qsort and a
 * fixed private C comparator; process/reset allocate nothing or call back
 * into Rust. No original code/data is loaded. */
#include <math.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "ambience_one/lengths.h"
#include "ambience_one/network.h"

typedef struct {uint32_t length,pad;float *data;uint32_t cursor;float coefficient;} Amb1Allpass;
typedef struct {float input,output,pole;} Amb1Damping;
typedef struct {
    uint32_t initialized,failed,rate,capacity,remaining,last_tail,tail;
    float dry,wet,gain;
    Amb1Allpass allpass[4];
    Amb1Feedback feedback[16];
    Amb1Damping damping[2];
    float scratch[4][256];
} NeteaseAmbienceOne;

#define AMB1_STORAGE_LIMIT ((size_t)4194304)
static int amb1_geometry(uint32_t rate,uint32_t lengths[16][4],uint32_t extra[16],uint32_t ap[4],size_t *count) {
    if(rate<8000 || rate>384000 || amb1_bank1_lengths(rate,lengths,extra) || amb1_allpass_lengths(rate,ap))return -1;
    uint64_t total=0;
    for(unsigned i=0;i<4;i++)total+=2*(uint64_t)((ap[i]+3)&~3u);
    for(unsigned i=0;i<16;i++) {
        total+=(extra[i]+3)&~3u;
        for(unsigned j=0;j<4;j++)total+=(lengths[i][j]+3)&~3u;
    }
    if(!total || total>AMB1_STORAGE_LIMIT)return -1;
    *count=(size_t)total;return 0;
}
size_t cumulus_ambience_one_storage(uint32_t rate) {
    uint32_t lengths[16][4],extra[16],ap[4];size_t count=0;
    return amb1_geometry(rate,lengths,extra,ap,&count)?0:count;
}
static float amb1_high_damping(uint32_t delay,uint32_t rate) {
    double one_minus=1.0-(double)10.0f*(double)10.0f;
    double exponent=(double)delay*(-3.0),denominator=(double)1.0f*(double)rate;
    exponent=exponent/denominator;
    double g=pow(10.0,exponent);g=log10(g);g=g*0.5756462732485116;
    if(g*one_minus>1.0)one_minus=1.0/g;
    return (float)sqrt(1.0-one_minus);
}
static void amb1_coefficients(float out[3],uint32_t delay,float ratio,uint32_t rate) {
    double exponent=(double)delay*(-3.0),denominator=(double)1.0f*(double)rate;
    exponent=exponent/denominator;
    double g=pow(10.0,exponent),logged=log10(g),scale=logged*0.5756462732485116;
    double fraction=1.0-(double)ratio*(double)ratio;scale=scale*fraction;
    if(scale>0.999)scale=0.999;
    out[0]=0.0f;out[1]=(float)((1.0-scale)*g);out[2]=(float)(-scale);
}
int cumulus_ambience_one_init(NeteaseAmbienceOne *s,uint32_t rate,uint32_t capacity,float *storage,size_t count) {
    uint32_t lengths[16][4],extra[16],ap[4];size_t needed=0;
    if(!s || !storage || capacity<4 || capacity>4096 || capacity%4 ||
       amb1_geometry(rate,lengths,extra,ap,&needed) || count!=needed)return -1;
    memset(s,0,sizeof(*s));memset(storage,0,count*sizeof(float));
    s->rate=rate;s->capacity=capacity;s->remaining=UINT32_MAX;s->tail=rate;
    s->gain=1.0f/sqrtf(16.0f);
    volatile float dry_db=-96.3f,wet_db=-18.0f;
    s->dry=powf(10.0f,dry_db*0.05f);s->wet=powf(10.0f,wet_db*0.05f-0.15f);
    for(unsigned i=0;i<2;i++) {
        float reciprocal=6.283185482025146484375f/(float)rate;
        s->damping[i].pole=1.0f-reciprocal*40.0f;
    }
    float *at=storage;
    for(unsigned i=0;i<4;i++) {
        s->allpass[i].length=ap[i];s->allpass[i].data=at;at+=2*((ap[i]+3)&~3u);
        float c=100.0f*0.02472119964659214f-(float)(3-i)*0.6180300116539001f;
        if(c<0)c=0;
        if(c>=0.6180300116539001f)c=0.6180300116539001f;
        s->allpass[i].coefficient=c;
    }
    for(unsigned i=0;i<16;i++) {
        float ratio=amb1_high_damping(lengths[i][3],rate);
        for(unsigned j=0;j<4;j++) {
            Amb1Delay *d=&s->feedback[i].ring[j];d->length=lengths[i][j];d->data=at;at+=(d->length+3)&~3u;
            amb1_coefficients(s->feedback[i].filter[j],d->length,ratio,rate);
            for(unsigned k=0;k<3;k++)if(!isfinite(s->feedback[i].filter[j][k]))return -1;
        }
        s->feedback[i].delay.length=extra[i];s->feedback[i].delay.data=at;at+=(extra[i]+3)&~3u;
    }
    if((size_t)(at-storage)!=count || !isfinite(s->dry) || !isfinite(s->wet))return -1;
    s->initialized=1;return 0;
}
int netease_ambience_one_reset(NeteaseAmbienceOne *s) {
    if(!s || !s->initialized || s->failed)return -1;
    for(unsigned i=0;i<4;i++)memset(s->allpass[i].data,0,s->allpass[i].length*8);
    for(unsigned i=0;i<16;i++) {
        amb1_feedback_reset(&s->feedback[i]);
        memset(s->feedback[i].delay.data,0,s->feedback[i].delay.length*4);
    }
    for(unsigned i=0;i<2;i++){s->damping[i].input=0;s->damping[i].output=0;}
    return 0;
}
static void amb1_delay(Amb1Delay *s,float *x,uint32_t n) {
    for(uint32_t i=0;i<n;i++) {
        float v=s->data[s->cursor];s->data[s->cursor++]=x[i];x[i]=v;
        if(s->cursor==s->length)s->cursor=0;
    }
}
static int amb1_allpass(Amb1Allpass *s,float *x,uint32_t n) {
    for(uint32_t i=0;i<n;i++) {
        float *p=s->data+2*s->cursor,old=p[0];p[0]=x[i];
        float y=x[i]-p[1];y=y*s->coefficient;y=y+old;
        if(!isfinite(y))return -1;
        p[1]=y;x[i]=y;if(++s->cursor==s->length)s->cursor=0;
    }
    return 0;
}
static int amb1_damp(Amb1Damping *s,float *x,uint32_t n,float gain) {
    float in=s->input,out=s->output;
    for(uint32_t i=0;i<n;i++) {
        float a=out*s->pole,b=gain*x[i],v=b-in;v=v+a;
        if(!isfinite(v))return -1;
        x[i]=v;in=b;out=v;
    }
    s->input=in;s->output=out;return 0;
}
int netease_ambience_one_process(NeteaseAmbienceOne *s,float *planar,uint32_t *frames,uint32_t *end) {
    if(!s || !s->initialized || s->failed || !planar || !frames || !end || *frames>s->capacity || *end>1)return -1;
    for(size_t i=0;i<2*(size_t)s->capacity;i++)if(!isfinite(planar[i]))return -1;
    unsigned n=*frames;
    if(*end) {
        if(!s->remaining && !n)return 0;
        if(s->remaining!=UINT32_MAX && !n) {
            if(s->tail>s->last_tail)s->remaining+=s->tail-s->last_tail;
        } else s->remaining=s->tail;
        s->last_tail=s->tail;
        unsigned add=s->remaining<s->capacity-n?s->remaining:s->capacity-n;
        s->remaining-=add;
        for(unsigned c=0;c<2;c++)memset(planar+c*s->capacity+n,0,(s->capacity-n)*4);
        n=s->remaining?s->capacity:n+add;*end=s->remaining==0;
    } else s->remaining=UINT32_MAX;
    *frames=n;if(!n)return 0;
    const float norm=sqrtf(1.0f/2.0f);
    for(unsigned off=0;off<n;off+=256) {
        unsigned count=n-off<256?n-off:256,rounded=(count+3)&~3u;
        float *input=s->scratch[0],*delayed=s->scratch[1],*left=s->scratch[2],*right=s->scratch[3];
        memset(input,0,256*sizeof(float));
        for(unsigned i=0;i<rounded;i++){float x=planar[off+i]*norm;input[i]=x+input[i];}
        for(unsigned i=0;i<rounded;i++){float x=planar[s->capacity+off+i]*norm;input[i]=x+input[i];if(!isfinite(input[i]))goto failed;}
        memcpy(delayed,input,count*4);
        for(unsigned i=0;i<4;i++)if(amb1_allpass(&s->allpass[i],delayed,count))goto failed;
        memset(left,0,256*sizeof(float));memset(right,0,256*sizeof(float));
        for(unsigned i=0;i<16;i++) {
            amb1_delay(&s->feedback[i].delay,delayed,count);
            if(amb1_feedback(&s->feedback[i],delayed,i%2?right:left,count))goto failed;
        }
        float gain=s->gain*1.41421353816986083984375f;
        if(amb1_damp(&s->damping[0],left,count,gain) || amb1_damp(&s->damping[1],right,count,gain))goto failed;
        /* Bank1 rshape180 => width1/complement0. Equal old/current gains use
         * the original C6C00 stable branch, including its rounded prefix. */
        for(unsigned i=0;i<rounded;i++) {
            float a=left[i]*s->wet,b=planar[off+i]*s->dry,c=right[i]*0.0f;
            a=a+b;planar[off+i]=a+c;
            a=left[i]*0.0f;b=planar[s->capacity+off+i]*s->dry;c=right[i]*s->wet;
            a=a+b;planar[s->capacity+off+i]=a+c;
            if(!isfinite(planar[off+i]) || !isfinite(planar[s->capacity+off+i]))goto failed;
        }
    }
    return 0;
failed:
    s->failed=1;memset(planar,0,2*(size_t)s->capacity*sizeof(float));return -2;
}
int netease_ambience_one_snapshot(const NeteaseAmbienceOne *s,uint32_t *indices,float *history) {
    if(!s || !s->initialized || !indices || !history)return -1;
    indices[0]=s->remaining;indices[1]=s->last_tail;indices[2]=s->tail;
    for(unsigned i=0;i<4;i++)indices[3+i]=s->allpass[i].cursor;
    for(unsigned i=0;i<16;i++) {
        for(unsigned j=0;j<4;j++){indices[7+5*i+j]=s->feedback[i].ring[j].cursor;history[6+4*i+j]=s->feedback[i].filter[j][0];}
        indices[7+5*i+4]=s->feedback[i].delay.cursor;
    }
    for(unsigned i=0;i<2;i++){history[3*i]=s->damping[i].input;history[3*i+1]=s->damping[i].output;history[3*i+2]=s->damping[i].pole;}
    return 0;
}
