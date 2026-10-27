/* Layout queries keep private recovered structs out of Rust's public ABI. */
#include "netease_multiband.c"
#include "netease_rotate.c"

size_t cumulus_dsp_size(unsigned kind) {
    switch(kind) {
        case 0:return sizeof(NeteaseBiquad);
        case 1:return sizeof(NeteaseGeqStage);
        case 2:return sizeof(NeteaseBassTreble);
        case 3:return sizeof(NeteasePeq);
        case 4:return sizeof(NeteaseRotate);
        case 5:return sizeof(NeteaseRotateStage);
        case 6:return sizeof(NeteaseMultiband);
        default:return 0;
    }
}
size_t cumulus_dsp_alignment(unsigned kind) {
    switch(kind) {
        case 0:return _Alignof(NeteaseBiquad);
        case 1:return _Alignof(NeteaseGeqStage);
        case 2:return _Alignof(NeteaseBassTreble);
        case 3:return _Alignof(NeteasePeq);
        case 4:return _Alignof(NeteaseRotate);
        case 5:return _Alignof(NeteaseRotateStage);
        case 6:return _Alignof(NeteaseMultiband);
        default:return 0;
    }
}

int cumulus_dynamics_validate(const unsigned char *raw,size_t bytes) {
    NeteaseDynamicsConfig c;
    if(dynamics_decode(&c,raw,bytes))return -1;
    for(unsigned i=0;i<5;i++)if(!isfinite(1.0f/c.band[i].ratio))return -2;
    return 0;
}

/* Preflight every coefficient before the recovered setter can modify state.
 * In particular, fourth-order subnormal frequencies can make the original
 * coefficient helper fail without writing its output array. */
static int validate_crossovers(float rate,unsigned fourth,const unsigned char *raw,size_t bytes) {
    NeteaseDynamicsConfig c;
    if(cumulus_dynamics_validate(raw,bytes) || dynamics_decode(&c,raw,bytes))return -1;
    for(unsigned i=0;i<4;i++)for(unsigned kind=0;kind<3;kind++) {
        float coefficients[5];
        if(netease_crossover_coefficients(kind,fourth,rate,c.frequency[i],coefficients))return -2;
        for(unsigned n=0;n<5;n++)if(!isfinite(coefficients[n]))return -2;
    }
    return 0;
}

int cumulus_multiband_init(NeteaseMultiband *s,float rate,unsigned capacity,const unsigned char *raw,size_t bytes) {
    if(!raw || bytes!=171 || validate_crossovers(rate,raw[162],raw,bytes))return -1;
    return netease_multiband_init(s,rate,capacity,raw,bytes);
}

int cumulus_multiband_set(NeteaseMultiband *s,const unsigned char *raw,size_t bytes) {
    if(!s || !s->initialized || validate_crossovers(s->rate,s->crossover[0].fourth,raw,bytes))return -1;
    return netease_multiband_set(s,raw,bytes);
}
