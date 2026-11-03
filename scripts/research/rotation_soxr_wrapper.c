#define _GNU_SOURCE
/* Manual Rotate3D oracle: run only through rotation_soxr_replay.py, which
 * checks the official image hash and installs public synthetic response tables.
 * The host supplies the old isolated allocation/FFT setup and CRT substitutes.
 * DSP/resampler instructions are untouched; DllMain and network are unused. */
#include <sys/mman.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#define MS __attribute__((ms_abi))
static unsigned char *base;
static void patch(unsigned off, void *ptr) {
    unsigned char jump[]={0x48,0xb8,0,0,0,0,0,0,0,0,0xff,0xe0};
    memcpy(jump+2,&ptr,8);memcpy(base+off,jump,sizeof(jump));
}
static void *MS host_getenv(const char *s) { (void)s;return NULL; }
static void *MS host_calloc(size_t n,size_t size) { return calloc(n,size); }
static void *MS host_heapalloc(void *heap,unsigned flags,size_t size) { (void)heap; return flags&8 ? calloc(1,size) : malloc(size); }
static int MS host_heapfree(void *heap,unsigned flags,void *p) { (void)heap;(void)flags;free(p);return 1; }
static void *MS host_heaprealloc(void *heap,unsigned flags,void *p,size_t n) { (void)heap;(void)flags;return realloc(p,n); }
static int MS host_rand(void) { return 12345; }
static void putp(void *object,size_t offset,void *value){memcpy((char*)object+offset,&value,8);}
static void puti(void *object,size_t offset,uint32_t value){memcpy((char*)object+offset,&value,4);}
static void putf(void *object,size_t offset,float value){memcpy((char*)object+offset,&value,4);}
static void *fifo(void) {
    /* AudioFIFO 714B0 and its two byte rings 71FB0, preallocated beyond the
     * tested occupancy. The original read/write/reset functions still execute. */
    unsigned char *q=calloc(1,0x28);void **rings=calloc(2,8);
    if(!q || !rings)exit(10);
    for(unsigned i=0;i<2;i++) {
        unsigned char *ring=calloc(1,0x28),*data=calloc(65536,4);
        if(!ring || !data)exit(10);
        putp(ring,0,data);putp(ring,8,data);putp(ring,0x10,data);putp(ring,0x18,data+65536*4);
        rings[i]=ring;
    }
    putp(q,0,rings);puti(q,8,2);puti(q,12,4);puti(q,16,4);puti(q,20,2);
    puti(q,24,1);puti(q,32,65536);return q;
}
static void *plan(unsigned inverse) {
    unsigned char *p=calloc(1,0x78);uint16_t *rev=calloc(1024,2);
    void *scratch=calloc(1024,8);if(!p || !rev || !scratch)exit(10);
    puti(p,0,10);puti(p,4,inverse);putp(p,8,rev);putp(p,0x10,scratch);
    putp(p,0x30,base+0x12690);putp(p,0x38,base+0x12610);puti(p,0x40,1);
    typedef int (MS *permutation_fn)(int,int,int);
    for(int i=0;i<1024;i++) {
        int index=((permutation_fn)(base+0x74d90))(i,1024,inverse);
        rev[(-index)&1023]=(i&~3)|((i&1)<<1)|((i>>1)&1);
    }
    return p;
}
int main(int argc,char **argv) {
    if(argc!=3)return 2;
    uint32_t rate=strtoul(argv[2],NULL,10);
    if(rate<8000||rate>384000)return 2;
    base=mmap((void*)0x180000000,0x3a3000,PROT_READ|PROT_WRITE,
              MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED_NOREPLACE,-1,0);
    if(base==MAP_FAILED)return 3;
    FILE *f=fopen(argv[1],"rb");if(!f)return 4;
    size_t got=fread(base,1,0x3a3000,f);fclose(f);
        patch(0x1882e8,(void*)host_getenv);
    patch(0x1544a0,(void*)host_calloc);
    patch(0x1887a4,(void*)host_rand);
    void *heapalloc=host_heapalloc, *heapfree=host_heapfree, *heaprealloc=host_heaprealloc;
    memcpy(base+0x394258,&heapalloc,8);
    memcpy(base+0x394250,&heapfree,8);
    memcpy(base+0x394328,&heaprealloc,8);

    if(got!=0x3a2400 || mprotect(base+0x1000,0x1e2000,PROT_READ|PROT_EXEC))return 5;
    typedef void (MS *twiddle_fn)(int);
    for(int bits=4;bits<=10;bits++)((twiddle_fn)(base+0x73e60))(bits);
    void *forward=plan(0),*inverse=plan(1);
    uint32_t blocks;float angle;
    if(fread(&blocks,4,1,stdin)!=1 || !blocks || blocks>64 || fread(&angle,4,1,stdin)!=1 ||
       !isfinite(angle) || angle<0 || angle>=360)return 9;
    _Alignas(16) unsigned char object[0x2090]={0};
    typedef void* (MS *ctor_fn)(void*);
    ((ctor_fn)(base+0x23a40))(object);
    puti(object,0,44100);puti(object,4,2);puti(object,8,256);
    putp(object,0x2020,forward);putp(object,0x2028,inverse);putf(object,0x2084,angle);
    for(unsigned i=0;i<10;i++) {
        void *p=calloc(i<6?1024:256,i<6?8:4);if(!p)return 10;putp(object,0x2030+8*i,p);
    }
    typedef void (MS *speed_fn)(void*,float);
    typedef void (MS *reset_fn)(void*);
    ((speed_fn)(base+0x25b80))(object,40);
    unsigned capacity=256;_Alignas(16) unsigned char wrapper[0x70]={0};
    {
        if(fread(&capacity,4,1,stdin)!=1 || capacity<256 || capacity>4096)return 13;
        ((ctor_fn)(base+0x22b50))(wrapper);
        putp(wrapper,8,object);putp(wrapper,0x10,fifo());putp(wrapper,0x18,fifo());
        void **pointers=calloc(2,8),**dry=calloc(2,8);float *dry_data=calloc(2*capacity,4);
        if(!pointers || !dry || !dry_data)return 10;
        dry[0]=dry_data;dry[1]=dry_data+capacity;putp(wrapper,0x20,pointers);putp(wrapper,0x28,dry);
        puti(wrapper,0x34,capacity);puti(wrapper,0x38,rate);puti(wrapper,0x3c,2);
        if(rate!=44100) {
            typedef unsigned char (MS *init)(void*,uint64_t,uint64_t,uint64_t,uint32_t);
            typedef uint64_t (MS *bound)(void*,uint64_t);
            uint64_t *in=calloc(4,8),*out=calloc(4,8);
            if(!in||!out || !((init)(base+0x78770))(in,2,rate,44100,4)
               || !((init)(base+0x78770))(out,2,44100,rate,4))return 15;
            unsigned resample_capacity=((bound)(base+0x788c0))(in,capacity);
            void **tmp=calloc(2,8);float *plane=calloc(2*resample_capacity,4);
            if(!tmp||!plane||!resample_capacity)return 16;
            tmp[0]=plane;tmp[1]=plane+resample_capacity;
            putp(wrapper,0x50,in);putp(wrapper,0x58,out);
            putp(wrapper,0x60,tmp);puti(wrapper,0x68,resample_capacity);
        }
    }
    for(unsigned b=0;b<blocks;b++) {
        uint32_t valid,reset;float period;float *data=calloc(2*capacity,4);if(!data)return 10;
        if(fread(&valid,4,1,stdin)!=1 || valid>capacity || fread(&period,4,1,stdin)!=1 ||
           !isfinite(period) || fread(&reset,4,1,stdin)!=1 || reset>7 ||
           fread(data,4,2*capacity,stdin)!=2*capacity)return 11;
            typedef void (MS *bypass_fn)(void*,unsigned char);
            typedef void (MS *outer_fn)(void*,void*,void*,void*);
            unsigned char end=(reset>>1)&1;
            if(reset&4)((reset_fn)(base+0x23990))(wrapper);
            ((speed_fn)(base+0x23a20))(wrapper,period);
            ((bypass_fn)(base+0x23a10))(wrapper,reset&1);
            ((outer_fn)(base+0x22ea0))(wrapper,data,&end,&valid);
            uint32_t ended=end;
            if(fwrite(&valid,4,1,stdout)!=1 || fwrite(&ended,4,1,stdout)!=1 ||
               fwrite(object+0x2084,4,1,stdout)!=1 || fwrite(data,4,2*capacity,stdout)!=2*capacity)return 14;
            free(data);
    }
    return 0;
}
