#ifndef CUMULUS_AMBIENCE_ONE_NETWORK_H
#define CUMULUS_AMBIENCE_ONE_NETWORK_H
#include <math.h>
/* Recovered C5FA0/C6550: source operation order, not a generic FDN.
 * Prefix-four vs scalar wrap/tail ordering is intentionally separate. */
#include <stdint.h>
#include <string.h>
typedef struct { uint32_t length,pad0; float *data; uint32_t cursor,pad1; } Amb1Delay;
typedef struct { Amb1Delay ring[4]; float filter[4][3]; Amb1Delay delay; } Amb1Feedback;
static void amb1_feedback_reset(Amb1Feedback *s) {
    for(unsigned k=0;k<4;k++) { memset(s->ring[k].data,0,s->ring[k].length*4);s->filter[k][0]=0.0f; }
}
static int amb1_feedback(Amb1Feedback *s,const float *input,float *output,uint32_t frames) {
    float history[4];for(unsigned k=0;k<4;k++)history[k]=s->filter[k][0];
    uint32_t off=0;
    while(off<frames) {
        uint32_t n=frames-off;
        for(unsigned k=0;k<4;k++)if(s->ring[k].length-s->ring[k].cursor<n)n=s->ring[k].length-s->ring[k].cursor;
        uint32_t prefix=n&~3u;
        for(uint32_t p=0;p<prefix;p+=4) {
            float f[4][4];
            for(unsigned k=0;k<4;k++)for(unsigned j=0;j<4;j++) {
                float a=-s->filter[k][2]*history[k];
                float b=s->ring[k].data[s->ring[k].cursor+j]*s->filter[k][1];
                f[k][j]=b+a;history[k]=f[k][j];
                if(!isfinite(f[k][j]))return -1;
            }
            for(unsigned j=0;j<4;j++) {
                float a=f[0][j]+f[2][j],b=f[1][j]+f[3][j];
                output[off+p+j]=(a+output[off+p+j])-b;
                a=f[0][j]+f[1][j];a=a+f[2][j];a=a+f[3][j];a=a*(-0.5f);a=a+input[off+p+j];
                for(unsigned k=0;k<4;k++){float v=a+f[(k+1)%4][j];if(!isfinite(v))return -1;s->ring[k].data[s->ring[k].cursor+j]=v;}
            }
            for(unsigned k=0;k<4;k++)s->ring[k].cursor+=4;
        }
        for(uint32_t p=prefix;p<n;p++) {
            float f[4];for(unsigned k=0;k<4;k++) {
                float a=-s->filter[k][2]*history[k];
                float b=s->ring[k].data[s->ring[k].cursor]*s->filter[k][1];
                f[k]=b+a;history[k]=f[k];if(!isfinite(f[k]))return -1;
            }
            float t0=f[0]*1.0f,t1=f[1]*(-1.0f),t2=f[2]*1.0f,t3=f[3]*(-1.0f);
            float a=t0+t2,b=t1+t3;output[off+p]=(a+b)+output[off+p];
            t0=f[0]*(-0.5f);t1=f[1]*(-0.5f);t2=f[2]*(-0.5f);t3=f[3]*(-0.5f);
            a=t0+t2;b=t1+t3;a=a+b;
            for(unsigned k=0;k<4;k++) {
                float v=f[(k+1)%4]+a;v=v+input[off+p];
                if(!isfinite(v))return -1;
                s->ring[k].data[s->ring[k].cursor++]=v;
            }
        }
        for(unsigned k=0;k<4;k++)if(s->ring[k].cursor==s->ring[k].length)s->ring[k].cursor=0;
        off+=n;
    }
    for(unsigned k=0;k<4;k++)s->filter[k][0]=history[k];
    return 0;
}

#endif
