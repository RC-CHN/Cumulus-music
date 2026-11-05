#define _GNU_SOURCE
/* Opt-in Linux x86_64 research oracle. Run only with the hash-pinned mapped
 * image built by freeze_stereo_zero.py inside a network namespace. No DllMain,
 * services, sound output, private assets or product runtime dependency.
 * Only the fixed bank1 environment child is executed, not the complete SE graph.
 * Original Init/Reset/Process and math execute; allocator/block context, PE
 * mapping and TEB stack are adapters. Numeric outputs contain no pointers.
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
    (void)self;if(size>4*1024*1024)exit(20);
    void *p=calloc(1,size?size:1);if(!p)exit(21);for(unsigned i=0;i<used;i++)if(!owned[i]){owned[i]=p;return p;}if(used==512)exit(20);owned[used++]=p;return p;
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
    if(strcmp(argv[1],"process")!=0)return 2;
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
    uint32_t h[4];if(fread(h,4,4,stdin)!=4 || h[3]>1 || h[0]<8000 || h[0]>384000 || h[1]<4 || h[1]>4096 || h[1]%4 || h[2]>16384)return 7;
    uint32_t rate=h[0],capacity=h[1],blocks=h[2];
    _Alignas(16) unsigned char object[0x88]={0},parameters[0x5b0]={0},wrapper[0x50]={0};
    typedef void* (MS *ctor_fn)(void*);
    typedef uint32_t (MS *param_init_fn)(void*,void*,void*,uint32_t);
    typedef uint32_t (MS *init_fn)(void*,void*,void*,void*,void*);
    typedef uint32_t (MS *reset_fn)(void*);
    typedef void (MS *set_fn)(void*,float,float,unsigned char,int32_t);
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
    ((set_fn)(base+0xa85b0))(wrapper,0.0f,0.0f,0,1);
    unsigned char *environment;memcpy(&environment,object+0x68,8);
    for(unsigned i=0;i<blocks;i++) {
        uint32_t n,end,reset;
        if(fread(&n,4,1,stdin)!=1 || n>capacity || fread(&end,4,1,stdin)!=1 || end>1 ||
           fread(&reset,4,1,stdin)!=1 || reset>1 || fread(pcm,4,2*capacity,stdin)!=2*capacity)return 11;
        if(reset) {typedef void (MS *r_fn)(void*);((r_fn)(base+0xbe020))(environment);}
        unsigned before=_mm_getcsr()&0x8000u;
        struct { float *pcm; uint32_t format,state; uint16_t capacity,frames; uint32_t pad; } block={pcm,0x3102,end?0x11:0x2d,(uint16_t)capacity,(uint16_t)n,0};
        typedef void (MS *env_process_fn)(void*,void*);
        ((env_process_fn)(base+0xb6eb0))(environment,&block);
        n=block.frames;end=block.state==0x11;
        if((_mm_getcsr()&0x8000u)!=before)return 15;
        if(fwrite(&n,4,1,stdout)!=1 || fwrite(&end,4,1,stdout)!=1 ||
           fwrite(pcm,4,2*capacity,stdout)!=2*capacity)return 12;
        uint32_t indices[87];float history[70];
        memcpy(indices,environment+0x118,12);
        for(unsigned k=0;k<4;k++)memcpy(indices+3+k,environment+8+24*k+16,4);
        unsigned char *nodes;memcpy(&nodes,environment+0x140,8);
        for(unsigned k=0;k<16;k++) {
            unsigned char *node=nodes+k*0xa8;
            for(unsigned j=0;j<4;j++) {
                memcpy(indices+7+5*k+j,node+24*j+16,4);
                memcpy(history+6+4*k+j,node+0x60+12*j,4);
            }
            memcpy(indices+7+5*k+4,node+0x90+16,4);
        }
        memcpy(history,environment+0x70,24);
        if(fwrite(indices,4,87,stdout)!=87 || fwrite(history,4,70,stdout)!=70)return 12;
    }
    free(pcm);for(unsigned i=0;i<used;i++)free(owned[i]);return 0;
}
