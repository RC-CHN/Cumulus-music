/* Explicit fixed local old-schema equalizer scope; no remote material/preset,
 * RVB-on, normalization, rotation or general optional SDK effect.
 * Includes the real first off-wrapper transitions in original graph order.
 * The caller still supplies the independently recovered final B0 node.
 */
#ifdef _MSC_VER
/* The SE arena deliberately requires 16-byte alignment inside this object. */
#pragma warning(push)
#pragma warning(disable: 4324)
#endif
typedef struct {
    NeteaseLocalOff off;
    NeteaseStereoOne stereo;
    unsigned bank_one;
} NeteaseLocalFixed;
#ifdef _MSC_VER
#pragma warning(pop)
#endif

static int local_fixed_init(NeteaseLocalFixed *s,uint32_t rate,unsigned cap,
                            float *arena,size_t count,unsigned initial_bank) {
    if(!s || !arena)return -1;
    memset(s,0,sizeof(*s));
    if(cumulus_local_off_init(&s->off,(float)rate,cap) ||
       cumulus_stereo_one_init(&s->stereo,rate,cap,arena,count))return -1;
    s->bank_one=initial_bank;
    if(!initial_bank)s->stereo.shared.dynamics[1].config.input=db_gain(1.5f);
    return netease_stereo_one_bypass(&s->stereo,1);
}
int cumulus_local_fixed_init(NeteaseLocalFixed *s,uint32_t rate,unsigned cap,
                             float *arena,size_t count) {
    /* Existing graph starts after the 1BBA0 defaults; cold-source edge
     * semantics intentionally remain separate from outer device Init. */
    return local_fixed_init(s,rate,cap,arena,count,0);
}
int cumulus_local_fixed_master_init(NeteaseLocalFixed *s,uint32_t rate,float *arena,size_t count) {
    /* Outer Init before material apply: default bank1, SE wrapper off.
     * The Rust owner performs the native node reset before applying material. */
    if(rate!=44100 && rate!=48000)return -1;
    return local_fixed_init(s,rate,2048,arena,count,1);
}
int netease_local_fixed_parameters(NeteaseLocalFixed *s,float presence,float stereoizer,unsigned enabled) {
    if(!s || !s->off.capacity || enabled>1 || !isfinite(presence) || !isfinite(stereoizer) ||
       presence < -10 || presence > 10 || stereoizer < 0 || stereoizer > 10)return -1;
    if(enabled) {
        if(netease_stereo_one_parameters(&s->stereo,presence,stereoizer,1))return -1;
        /* A85B0 selects bank1 before the separate A8550 enable. The prior
         * input gain is retained, so an earlier bank0 block can still ramp. */
        s->stereo.shared.dynamics[1].config.input=db_gain(1.0f);s->bank_one=1;
    }
    /* On=false preserves parameters/bank as the original cloud parser does. */
    return netease_stereo_one_bypass(&s->stereo,!enabled);
}
/* Outer 68770/6B960(true) in the fixed defaults scope: the only
 * master-sensitive active node is SE. Keep its controls/bank and history,
 * leave wrapper previous untouched, and let B0 continue with its defaults. */
int netease_local_fixed_master_off(NeteaseLocalFixed *s) {
    if(!s || !s->off.capacity)return -1;
    return netease_stereo_one_bypass(&s->stereo,1);
}

/* A85B0 resets the SE plugin on bank change without synchronizing the
 * wrapper edge. A8550 can reset once more on the separate enable call. */
static int local_fixed_se_defaults(NeteaseLocalFixed *s) {
    if(netease_stereo_one_parameters(&s->stereo,0,0,1))return -1;
    if(s->bank_one) {
        s->bank_one=0;s->stereo.shared.dynamics[1].config.input=db_gain(1.5f);
        if(netease_stereo_one_reset(&s->stereo))return -1;
    }
    return netease_stereo_one_bypass(&s->stereo,1);
}
static int local_fixed_se_enable(NeteaseLocalFixed *s,float presence,float stereoizer) {
    if(netease_stereo_one_parameters(&s->stereo,presence,stereoizer,1))return -1;
    s->bank_one=1;s->stereo.shared.dynamics[1].config.input=db_gain(1.0f);
    if(netease_stereo_one_reset(&s->stereo))return -1;
    return netease_stereo_one_bypass(&s->stereo,0);
}
/* Existing SE-only master path: non-SE values are immutable defaults. */
int netease_local_fixed_master_material(NeteaseLocalFixed *s,float presence,float stereoizer) {
    if(!s || !s->off.capacity || !isfinite(presence) || !isfinite(stereoizer) ||
       presence < -10 || presence > 10 || stereoizer < 0 || stereoizer > 10)return -1;
    if(local_fixed_se_defaults(s))return -1;
    return local_fixed_se_enable(s,presence,stereoizer);
}

static int local_equalizer_format(const NeteaseLocalFixed *s) {
    return s && s->off.capacity==2048 &&
           (s->off.geq.core.rate==44100.0f || s->off.geq.core.rate==48000.0f) &&
           s->off.geq.core.channels==2;
}
static int local_equalizer_tone(NeteasePeq *bt,float bass,float treble) {
    /* A2DF0/processor slot 50 uses the shared nine-slot PEQ, not the
     * standalone BT ctor. The middle band stays disabled. */
    return netease_peq_band(bt,0,7,bass,120.0f,0.5f,1) ||
           netease_peq_band(bt,2,8,treble,6000.0f,0.5f,1);
}
int netease_local_equalizer_material(NeteaseLocalFixed *s,const float *geq,
                                    int32_t bass,int32_t treble,int32_t presence,int32_t stereoizer) {
    if(!local_equalizer_format(s) || bass< -10 || bass>10 || treble< -10 || treble>10 ||
       presence< -10 || presence>10 || stereoizer<0 || stereoizer>10)return -1;
    if(geq)for(unsigned i=0;i<10;i++)if(!isfinite(geq[i]) || geq[i]< -12 || geq[i]>12)return -1;
    /* 1BBA0: restore off/defaults before the old-schema component setters.
     * Turning a wrapper off does not synchronize its previous flag. */
    const float flat[10]={0};
    if(netease_geq_stage_bypass(&s->off.geq,1) || netease_geq_stage_set(&s->off.geq,flat) ||
       netease_peq_bypass(&s->off.bt,1) || local_equalizer_tone(&s->off.bt,0,0) ||
       local_fixed_se_defaults(s))return -1;
    if(geq && (netease_geq_stage_set(&s->off.geq,geq) ||
               netease_geq_stage_bypass(&s->off.geq,0)))return -1;
    if((bass || treble) && (local_equalizer_tone(&s->off.bt,(float)bass,(float)treble) ||
                           netease_peq_bypass(&s->off.bt,0)))return -1;
    return (presence || stereoizer)?local_fixed_se_enable(s,(float)presence,(float)stereoizer):0;
}
int netease_local_equalizer_master_off(NeteaseLocalFixed *s) {
    if(!local_equalizer_format(s))return -1;
    /* 1C590 target bypass keeps actual parameters, histories and previous
     * edges. B0 stays alive with its already-default settings in Rust. */
    return netease_stereo_one_bypass(&s->stereo,1) || netease_geq_stage_bypass(&s->off.geq,1) ||
           netease_peq_bypass(&s->off.bt,1)?-1:0;
}

int netease_local_fixed_reset(NeteaseLocalFixed *s) {
    if(!s || !s->off.capacity || netease_local_off_reset(&s->off) ||
       netease_stereo_one_reset(&s->stereo))return -1;
    /* A8510 wraps plugin B3990 then synchronizes the wrapper edge. */
    s->stereo.shared.previous=s->stereo.shared.bypass;return 0;
}
static int local_fixed_bank_zero(NeteaseStereoZero *s,float *pcm,unsigned frames,float *dry) {
    unsigned changing=s->bypass!=s->previous;
    if(changing)memcpy(dry,pcm,2*(size_t)s->capacity*sizeof(float));
    if(changing || !s->bypass) {
        unsigned csr=begin_plugin();
        if(stereo_shared_prepare(s,pcm,frames,1)){end_plugin(csr);return -1;}
        unsigned vector=frames&~3u;
        for(unsigned i=0;i<frames;i++) {
            float mid=pcm[i],side=pcm[s->capacity+i];
            pcm[i]=i<vector?mid*0.5f+side*0.5f:(mid+side)*0.5f;
            pcm[s->capacity+i]=(mid-side)*0.5f;
        }
        end_plugin(csr);
    }
    if(changing)blend_channels(pcm,dry,2,s->capacity,s->bypass);
    s->previous=s->bypass;return 0;
}
int netease_local_fixed_process(NeteaseLocalFixed *s,float *pcm,unsigned *frames,unsigned *end,float *dry) {
    if(!s || !frames || !end || *end>1 || local_off_buffers(&s->off,pcm,dry,*frames))return -1;
    if(netease_local_off_before(&s->off,pcm,*frames,dry))return -1;
    if(s->bank_one) {
        if(netease_stereo_one_process(&s->stereo,pcm,frames,end,dry))return -1;
    } else if(local_fixed_bank_zero(&s->stereo.shared,pcm,*frames,dry))return -1;
    return netease_local_off_after(&s->off,pcm,*frames,dry);
}
