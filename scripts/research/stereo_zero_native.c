#define _GNU_SOURCE
/* Opt-in Linux x86_64 research oracle. Run only with the hash-pinned mapped
 * image built by freeze_stereo_zero.py inside a network namespace. No DllMain,
 * services, sound output, private assets or product runtime dependency.
 * Only the reachable ambience=0 branch is accepted. Original child graph and
 * math execute; allocator/block context, PE mapping and TEB stack are adapters.
 */
#include <sys/mman.h>
#include <sys/syscall.h>
#include <asm/prctl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <xmmintrin.h>
#define MS __attribute__((ms_abi))
struct adapter {void **vtable;uint32_t frames;};
static void *owned[512];static unsigned used;
static void *MS allocate(void *self,size_t size) {
    (void)self;if(size>4*1024*1024 || used==512)exit(20);
    void *p=calloc(1,size?size:1);if(!p)exit(21);owned[used++]=p;return p;
}
static void MS release(void *self,void *p) {
    (void)self;if(!p)return;
    for(unsigned i=0;i<used;i++)if(owned[i]==p){free(p);owned[i]=NULL;return;}
    exit(22);
}
static void *MS global(void *self){return self;}
static uint32_t MS frames(void *self){return ((struct adapter*)self)->frames;}
static void putp(void *object,size_t offset,void *p){memcpy((char*)object+offset,&p,8);}
static void puti(void *object,size_t offset,uint32_t value){memcpy((char*)object+offset,&value,4);}
int main(int argc,char **argv) {
    if(argc!=3)return 2;
    int info=strcmp(argv[1],"info")==0;
    if(!info && strcmp(argv[1],"process")!=0)return 2;
    unsigned char *base=mmap((void*)0x180000000,0x3a3000,PROT_READ|PROT_WRITE,
                             MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED_NOREPLACE,-1,0);
    if(base==MAP_FAILED)return 3;
    FILE *file=fopen(argv[2],"rb");if(!file)return 4;
    size_t got=fread(base,1,0x3a3000,file);fclose(file);
    if(got!=0x3a2400 || mprotect(base+0x1000,0x1e2000,PROT_READ|PROT_EXEC))return 5;
    pthread_attr_t attr;void *stack;size_t stack_size;
    if(pthread_getattr_np(pthread_self(),&attr) || pthread_attr_getstack(&attr,&stack,&stack_size))return 6;
    pthread_attr_destroy(&attr);uintptr_t teb[3]={0,0,(uintptr_t)stack};
    if(syscall(SYS_arch_prctl,ARCH_SET_GS,teb))return 6;
    uint32_t h[4];if(fread(h,4,4,stdin)!=4 || h[3]>1 || h[0]<8000 || h[0]>384000 || h[1]<4 || h[1]>4096 || h[1]%4 || h[2]>4096)return 7;
    uint32_t rate=h[0],capacity=h[1],blocks=h[2];
    _Alignas(16) unsigned char object[0x88]={0},parameters[0x5b0]={0},wrapper[0x50]={0};
    typedef void* (MS *ctor_fn)(void*);
    typedef uint32_t (MS *param_init_fn)(void*,void*,void*,uint32_t);
    typedef uint32_t (MS *init_fn)(void*,void*,void*,void*,void*);
    typedef uint32_t (MS *reset_fn)(void*);
    typedef void (MS *set_fn)(void*,float,float,unsigned char,int32_t);
    typedef void (MS *bypass_fn)(void*,unsigned char);
    typedef void (MS *process_fn)(void*,void*,void*,void*);
    void *alloc_vt[3]={NULL,allocate,release},*context_vt[3]={NULL,global,frames};
    struct adapter allocator={alloc_vt,0},context={context_vt,capacity};
    ((ctor_fn)(base+0xb2030))(object);((ctor_fn)(base+0xb1780))(parameters);
    uint32_t bytes;memcpy(&bytes,base+0x29c508,4);
    if(((param_init_fn)(base+0xb1ae0))(parameters,&allocator,base+0x29c510,bytes)!=1)return 8;
    uint32_t format[3]={rate,0x3102,0};
    if(((init_fn)(base+0xb34c0))(object,&allocator,&context,parameters,format)!=1)return 9;
    if(((reset_fn)(base+0xb3990))(object)!=1)return 10;
    puti(wrapper,0x20,0x3102);memcpy(wrapper+0x28,&capacity,2);
    putp(wrapper,0x38,object);putp(wrapper,0x40,parameters);
    float *pcm=calloc(2*capacity,4);if(!pcm)return 21;
    _mm_setcsr((_mm_getcsr()&~0x8000u)|(h[3]?0x8000u:0));
    for(unsigned i=0;i<blocks;i++) {
        uint32_t n,bypass,end,shaper,reset;int32_t ambience;float presence,stereoizer;
        if(fread(&n,4,1,stdin)!=1 || !n || n>capacity || fread(&bypass,4,1,stdin)!=1 || bypass>1 ||
           fread(&end,4,1,stdin)!=1 || end>1 || fread(&presence,4,1,stdin)!=1 ||
           fread(&stereoizer,4,1,stdin)!=1 || fread(&shaper,4,1,stdin)!=1 || shaper>1 ||
           fread(&ambience,4,1,stdin)!=1 || ambience!=0 || fread(&reset,4,1,stdin)!=1 || reset>1 || fread(pcm,4,2*capacity,stdin)!=2*capacity)return 11;
        ((set_fn)(base+0xa85b0))(wrapper,presence,stereoizer,shaper,ambience);
        ((bypass_fn)(base+0xa8550))(wrapper,bypass);
        if(reset && ((reset_fn)(base+0xb3990))(object)!=1)return 14;
        unsigned char ended=end;
        unsigned before=_mm_getcsr()&0x8000u;
        ((process_fn)(base+0xa80d0))(wrapper,pcm,&ended,&n);end=ended;
        if((_mm_getcsr()&0x8000u)!=before)return 15;
        if(!info && (fwrite(&n,4,1,stdout)!=1 || fwrite(&end,4,1,stdout)!=1 ||
                     fwrite(pcm,4,2*capacity,stdout)!=2*capacity))return 12;
        if(!info) {
            if(fwrite(object+0x80,4,2,stdout)!=2)return 12;
            for(unsigned channel=0;channel<2;channel++) {
                unsigned char *dynamics;memcpy(&dynamics,object+0x58+channel*8,8);
                for(unsigned band=0;band<5;band++) { void *history;memcpy(&history,dynamics+0x48+band*0x40+0x20,8);if(fwrite(history,4,2,stdout)!=2)return 12; }
            }
        }
    }
    if(info) {
        /* Raw layout for offline field analysis, including pinned vtables.
         * Callers compare decoded numeric fields, not host pointer bytes. */
        fwrite(parameters,1,sizeof(parameters),stdout);fwrite(object+0x80,4,2,stdout);
    }
    free(pcm);for(unsigned i=0;i<used;i++)free(owned[i]);return 0;
}
