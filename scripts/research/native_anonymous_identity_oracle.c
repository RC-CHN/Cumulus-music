#define _GNU_SOURCE
/* Pinned x64 encodeAnonymousId core, no loader/DllMain/CEF/network.
 * Only MSVC std::string, CRT memory and debug-check seams are host adapters.
 * XOR/MD5/Base64/URL replacements remain original PE instructions. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <unistd.h>
#include <ucontext.h>

#define MS __attribute__((ms_abi))
#define IMAGE ((uintptr_t)0x180000000ULL)
#define MAX 16384
typedef struct { union { unsigned char small[16]; unsigned char *heap; } data; size_t size, capacity; } Str;
typedef struct { const unsigned char *bytes; size_t size; } View;
_Static_assert(sizeof(Str)==32,"MSVC x64 string");
static void *allocated[2048];
static size_t active;
static void die(void) { _Exit(21); }
static void *alloc(size_t n) { if(n>MAX || active==2048)die();void *p=calloc(1,n?n:1);if(!p)die();allocated[active++]=p;return p; }
static void release(void *p) { if(!p)return;for(size_t i=0;i<active;i++)if(allocated[i]==p){free(p);allocated[i]=NULL;return;}die(); }
static unsigned char *bytes(Str *s) { if(s->size>MAX || s->capacity>MAX || s->size>s->capacity)die();return s->capacity<16?s->data.small:s->data.heap; }
static Str *empty(Str *s) { memset(s,0,sizeof(*s));s->capacity=15;return s; }
static void clear(Str *s) { if(s->capacity>=16)release(s->data.heap);empty(s); }
static Str *assign(Str *s,const void *p,size_t n,int initialized) { if(n>MAX)die();unsigned char *copy=alloc(n+1);if(n)memcpy(copy,p,n);if(initialized)clear(s);else empty(s);if(n<16){memcpy(s->data.small,copy,n);release(copy);}else{s->data.heap=copy;s->capacity=n;}s->size=n;return s; }
static Str *MS ctor(Str *s) { return empty(s); }
static unsigned char MS is_empty(Str *s) { return s->size==0; }
static Str *MS from_c(Str *s,const char *p) { return assign(s,p,strlen(p),0); }
static Str *MS copy_ctor(Str *s,Str *p) { return assign(s,bytes(p),p->size,0); }
static Str *MS copy_assign(Str *s,Str *p) { return s==p?s:assign(s,bytes(p),p->size,1); }
static void MS dtor(Str *s) { clear(s); }
static size_t MS length(Str *s) { (void)bytes(s);return s->size; }
static Str *MS resize(Str *s,size_t n,unsigned char fill) { if(n>MAX)die();size_t old=s->size;unsigned char *p=alloc(n+1);if(old&&n)memcpy(p,bytes(s),old<n?old:n);if(n>old)memset(p+old,fill,n-old);assign(s,p,n,1);release(p);return s; }
static unsigned char *MS index_at(Str *s,size_t index) { if(index>=s->size)die();return bytes(s)+index; }
static View *MS view(View *out,Str *s) { out->bytes=bytes(s);out->size=s->size;return out; }
static Str *MS grow_empty(Str *s,size_t n,size_t ignored,size_t count,unsigned char fill) { (void)ignored;(void)count;return resize(s,n,fill); }
static Str *MS append(Str *s,const void *p,size_t n) { size_t old=s->size;if(old>MAX-n)die();unsigned char *tmp=alloc(old+n+1);memcpy(tmp,bytes(s),old);memcpy(tmp+old,p,n);assign(s,tmp,old+n,1);release(tmp);return s; }
static Str *MS insert(Str *s,size_t pos,const void *p,size_t n) { size_t old=s->size;if(pos>old||old>MAX-n)die();unsigned char *tmp=alloc(old+n+1);memcpy(tmp,bytes(s),pos);memcpy(tmp+pos,p,n);memcpy(tmp+pos+n,bytes(s)+pos,old-pos);assign(s,tmp,old+n,1);release(tmp);return s; }
static void *MS set_mem(void *out,int c,size_t n) { if(n>MAX)die();return memset(out,c,n); }
static void *MS copy_mem(void *out,const void *in,size_t n) { if(n>MAX)die();return memmove(out,in,n); }
static void MS delete_mem(void *p) { release(p); }
static void trap(int signal,siginfo_t *info,void *context) { (void)signal;ucontext_t *c=context;char msg[128];int n=snprintf(msg,sizeof(msg),"trap rip=%llx addr=%llx\n",(unsigned long long)c->uc_mcontext.gregs[REG_RIP],(unsigned long long)(uintptr_t)info->si_addr);if(write(2,msg,(size_t)n)!=n)_Exit(81);_Exit(80); }
static uint32_t u32(const unsigned char *p) { uint32_t v;memcpy(&v,p,4);return v; }
static void hook(unsigned char *base,size_t rva,void *function) { unsigned char *p=base+rva;p[0]=0x48;p[1]=0xb8;memcpy(p+2,&function,8);p[10]=0xff;p[11]=0xe0; }
int main(int argc,char **argv) {
    if(argc!=2)return 2;
    struct rlimit cpu={2,2},mem={256*1024*1024,256*1024*1024},core={0,0};
    if(setrlimit(RLIMIT_CPU,&cpu)||setrlimit(RLIMIT_AS,&mem)||setrlimit(RLIMIT_CORE,&core))return 3;
    alarm(4);
    struct sigaction action={.sa_sigaction=trap,.sa_flags=SA_SIGINFO};sigemptyset(&action.sa_mask);sigaction(SIGSEGV,&action,NULL);sigaction(SIGILL,&action,NULL);
    FILE *f=fopen(argv[1],"rb");if(!f)return 4;fseek(f,0,SEEK_END);long size=ftell(f);rewind(f);if(size<4096||size>64*1024*1024)return 5;
    unsigned char *file=malloc((size_t)size);if(!file||fread(file,1,(size_t)size,f)!=(size_t)size)return 6;fclose(f);
    uint32_t pe=u32(file+60);if(pe>(uint32_t)size-512||memcmp(file+pe,"PE\0\0",4))return 7;
    uint16_t sections,opt_size;memcpy(&sections,file+pe+6,2);memcpy(&opt_size,file+pe+20,2);uint32_t image_size=u32(file+pe+24+56),headers=u32(file+pe+24+60);
    if(image_size!=0x2727000||headers>(uint32_t)size||sections>32)return 8;
    unsigned char *base=mmap((void*)IMAGE,image_size,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED_NOREPLACE,-1,0);if(base==MAP_FAILED)return 9;
    memcpy(base,file,headers);
    for(unsigned i=0;i<sections;i++){unsigned char *s=file+pe+24+opt_size+40*i;uint32_t rva=u32(s+12),n=u32(s+16),off=u32(s+20);if(rva>image_size||n>image_size-rva||off>(uint32_t)size||n>(uint32_t)size-off)return 10;memcpy(base+rva,file+off,n);}
    free(file);
    hook(base,0x06f8a0,ctor);hook(base,0x06f920,is_empty);hook(base,0x070840,from_c);hook(base,0x06fd00,copy_ctor);hook(base,0x065e70,copy_assign);hook(base,0x070a30,dtor);
    hook(base,0x065e10,length);hook(base,0x065d60,resize);hook(base,0x068d80,index_at);hook(base,0x06f2e0,index_at);hook(base,0x098860,view);hook(base,0x06a360,grow_empty);hook(base,0x06fb40,append);hook(base,0x06b830,insert);
    hook(base,0xc51bf0,set_mem);hook(base,0xc51540,copy_mem);hook(base,0xc4d750,delete_mem);
    /* MSVC debug/security check helpers preserve the result register. A
     * one-byte RET preserves it too; a normal absolute jump would clobber it. */
    base[0xc4d23c]=0xc3;base[0xc4df4c]=0xc3;base[0xc4e140]=0xc3;
    if(mprotect(base+0x1000,0x19b1000,PROT_READ|PROT_EXEC))return 11;
    uint32_t count;if(fread(&count,4,1,stdin)!=1||count>128)return 12;
    for(uint32_t i=0;i<count;i++){
        uint32_t n;if(fread(&n,4,1,stdin)!=1||n>4096)return 13;unsigned char data[4096];if(fread(data,1,n,stdin)!=n)return 14;
        Str input,output;assign(&input,data,n,0);memset(&output,0,sizeof(output));
        typedef Str *(MS *Encode)(Str*,Str*);Str *returned=((Encode)(base+0x33c1d0))(&output,&input);if(returned!=&output||output.size>MAX)return 15;
        uint32_t length=(uint32_t)output.size;if(fwrite(&length,4,1,stdout)!=1||fwrite(bytes(&output),1,length,stdout)!=length)return 16;
        clear(&input);clear(&output);
        for(size_t j=0;j<active;j++)if(allocated[j])return 17;
        active=0;
    }
    return 0;
}
