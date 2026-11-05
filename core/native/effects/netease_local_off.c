/* Fixed default-off wrappers of the recovered stereo local-material lane.
 * This is deliberately NOT a general implementation of those plugins.
 * A1CA0/+58,+60: default floor=0 and ratio=4 make the linked detector's
 * reduction max(nonnegative_envelope * -.75, 0) exactly zero. The original
 * fast 10^0 is 0.9990389943, not 1; preserve that multiply and full-capacity
 * wrapper edge. A3A50/+68 uses nine disabled bands. A49F0/+78 and A89A0/+20
 * retain their original dry==wet activation blend in the verified off scope.
 * Native Reset synchronizes previous=target; it never consumes fake audio.
 */
typedef struct {
    NeteaseGeqStage geq;
    NeteasePeq bt,peq;
    unsigned capacity,pre58_previous,pre60_previous,post78_previous,rvb_previous;
} NeteaseLocalOff;

int cumulus_local_off_init(NeteaseLocalOff *s,float rate,unsigned capacity) {
    if(!s || !isfinite(rate) || (rate!=44100.0f && rate!=48000.0f && rate!=96000.0f) ||
       (capacity!=512 && capacity!=2048))return -1;
    memset(s,0,sizeof(*s));s->capacity=capacity;
    if(netease_geq_stage_init(&s->geq,rate,2,capacity) ||
       netease_geq_stage_bypass(&s->geq,1) ||
       netease_peq_init(&s->bt,rate,2,capacity,0,1) ||
       netease_peq_init(&s->peq,rate,2,capacity,0,1))return -1;
    /* Actual 1BBA0 defaults, not the parameter object's ctor defaults. */
    for(unsigned band=0;band<9;band++) {
        unsigned type=band==0?7:band==1?6:8;
        float bt_frequency=band==0?120.0f:band==1?1000.0f:band==2?6000.0f:5000.0f;
        float bt_q=band==0 || band==2?0.5f:band==1?1.0f:5.0f;
        float peq_frequency=band==0?100.0f:band==1?1000.0f:band==2?10000.0f:18000.0f;
        if(netease_peq_band(&s->bt,band,type,0,bt_frequency,bt_q,band==0 || band==2) ||
           netease_peq_band(&s->peq,band,type,0,peq_frequency,1.0f,0))return -1;
    }
    return netease_peq_bypass(&s->bt,1) || netease_peq_bypass(&s->peq,1)?-1:0;
}

int netease_local_off_reset(NeteaseLocalOff *s) {
    if(!s || !s->capacity)return -1;
    if(netease_geq_stage_reset(&s->geq) || netease_peq_reset(&s->bt) ||
       netease_peq_reset(&s->peq))return -1;
    s->pre58_previous=s->pre60_previous=s->post78_previous=s->rvb_previous=1;
    return 0;
}

static void default_off_detector(float *planar,unsigned cap,size_t frames,float *dry) {
    memcpy(dry,planar,2*(size_t)cap*sizeof(float));
    unsigned csr=begin_plugin();float gain=fast_power10(0.0f);
    for(unsigned ch=0;ch<2;ch++) {
        float *channel=planar+ch*cap;
        /* AB050 clears the whole output and writes back only valid PCM.
         * Pre/post gain are both exactly 1 in this fixed configuration. */
        for(size_t i=0;i<frames;i++)channel[i]=channel[i]*gain;
        memset(channel+frames,0,(cap-frames)*sizeof(float));
    }
    end_plugin(csr);blend_channels(planar,dry,2,cap,1);
}

static int local_off_buffers(const NeteaseLocalOff *s,const float *pcm,const float *dry,size_t frames) {
    if(!s || !s->capacity || !pcm || !dry || frames>s->capacity)return -1;
    size_t bytes=2*(size_t)s->capacity*sizeof(float);
    uintptr_t a=(uintptr_t)pcm,b=(uintptr_t)dry;
    if(a>UINTPTR_MAX-bytes || b>UINTPTR_MAX-bytes || (a<b+bytes && b<a+bytes))return -1;
    return 0;
}
int netease_local_off_before(NeteaseLocalOff *s,float *pcm,size_t frames,float *dry) {
    if(local_off_buffers(s,pcm,dry,frames))return -1;
    if(!s->pre58_previous){default_off_detector(pcm,s->capacity,frames,dry);s->pre58_previous=1;}
    if(netease_geq_stage_process(&s->geq,pcm,frames,dry) ||
       netease_peq_process(&s->bt,pcm,frames,dry) ||
       netease_peq_process(&s->peq,pcm,frames,dry))return -1;
    if(!s->pre60_previous){default_off_detector(pcm,s->capacity,frames,dry);s->pre60_previous=1;}
    return 0;
}
int netease_local_off_after(NeteaseLocalOff *s,float *pcm,size_t frames,float *dry) {
    if(local_off_buffers(s,pcm,dry,frames))return -1;
    if(!s->post78_previous) {
        memcpy(dry,pcm,2*(size_t)s->capacity*sizeof(float));
        blend_channels(pcm,dry,2,s->capacity,1);s->post78_previous=1;
    }
    if(!s->rvb_previous) {
        memcpy(dry,pcm,2*(size_t)s->capacity*sizeof(float));
        blend_channels(pcm,dry,2,s->capacity,1);s->rvb_previous=1;
    }
    return 0;
}
