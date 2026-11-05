#!/usr/bin/env python3
"""Manual source-backed x64 SDK interpreter; not a Windows runtime.

Only the hash-pinned caller-supplied DLL supplies instructions. This module
models bounded reviewed host services; it supplies no DSP or math output.
"""
import sys,struct,json,hashlib,time,traceback,pefile
from pathlib import Path
from .image import NativeImage
from unicorn import Uc,UC_ARCH_X86,UC_MODE_64,UC_HOOK_CODE,UC_HOOK_MEM_INVALID,UC_PROT_READ,UC_PROT_WRITE,UC_PROT_EXEC,UcError
from unicorn.x86_const import *
BASE=0x180000000;HEAP=0x71000000;HEAPSIZE=64*1024*1024;STACK=0x70000000;STACKSIZE=4*1024*1024;STUB=0x76000000;STOP=STUB+0xf000
class VM:
 def __init__(self,dll_path):
  self.im=NativeImage(dll_path);assert hashlib.sha256(self.im.raw).hexdigest()=='c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be'
  self.mutable_sections=[(BASE+sec.VirtualAddress,max(sec.Misc_VirtualSize,sec.SizeOfRawData)) for sec in pefile.PE(data=self.im.raw).sections if sec.Characteristics&0x80000000]
  self.u=Uc(UC_ARCH_X86,UC_MODE_64);self.allocs={};self.families={};self.top=HEAP;self.crt_locks={};self.srw_locks=set();self.critical_locks={};self.exit_callbacks=[];self.fls={};self.import_events=[];self.os_functions={};self.protections={};self.last_error=0;self.events=[];self.current='setup';self.hooks={};self.lastbad=None
  for p,n in [(BASE,(len(self.im.data)+4095)&~4095),(HEAP,HEAPSIZE),(STACK,STACKSIZE),(STUB,65536)]:self.u.mem_map(p,n)
  self.u.mem_map(0x394000,0x3000);self.u.mem_write(0x394000,b'\xcc'*0x3000);self.u.mem_write(0x394c4c,b'\xc3')
  self.u.mem_write(BASE,self.im.data);self.u.mem_write(STUB,b'\xc3'*65536)
  self.u.mem_protect(BASE+0x1000,0x1e2000,UC_PROT_READ|UC_PROT_EXEC)
  self.u.reg_write(UC_X86_REG_GS_BASE,STUB+0x8000);self.ptr(STUB+0x8010,STACK)
  self.u.reg_write(UC_X86_REG_MXCSR,0x1f80);self.u.reg_write(UC_X86_REG_FPCW,0x37f);self.u.reg_write(UC_X86_REG_FPSW,0);self.u.reg_write(UC_X86_REG_FPTAG,0xffff)
  self.u.hook_add(UC_HOOK_MEM_INVALID,self.invalid)
  self.hook(STOP,lambda:self.u.emu_stop())
  # PE TLS template copied exactly as a single-module loader would do.
  self.tls=self.allocate(0x208);self.u.mem_write(self.tls,self.im.data[0x397000:0x397208]);self.tls_array=self.allocate(8);self.ptr(self.tls_array,self.tls);self.ptr(STUB+0x8058,self.tls_array)
  # Only reviewed runtime-adapter entry points; original text stays intact.
  for rva,fn in [(0x6c4e,self.memset),(0x9421,self.memcpy),(0x4bf6,self.memcpy),(0x1762,self.lock),(0x2284,self.unlock),(0x275c,self.crt_lock),(0x5899,self.crt_unlock),]:self.hook(BASE+rva,fn)
  self.hook(0x394c4c,self.filetime)
  for addr,fn in [(0x394a32,self.srw_acquire),(0x394a18,self.srw_release),(0x394ade,self.critical_enter),(0x394af6,self.critical_leave),(0x394a6a,lambda:self.ret(1)),(0x394b62,lambda:self.ret(self.reg(UC_X86_REG_RCX))),(0x394b72,lambda:self.ret(self.reg(UC_X86_REG_RCX))),(0x394f82,lambda:self.ret(self.last_error)),(0x394f92,self.set_last_error)]:
   self.u.mem_write(addr,b'\xc3');self.hook(addr,fn)
  self.hook(BASE+0x1a1668,self.locale_request);self.hook(BASE+0x4066,self.atexit);self.hook(BASE+0x1b728c,self.before_raise)
  for addr,fn in [(0x39500a,self.load_library),(0x394d58,self.get_proc),(0x3950c6,self.virtual_protect),(0x395082,self.fls_alloc),(0x39508e,self.fls_get),(0x39509c,self.fls_set),(0x39534e,lambda:self.ret(0x1000)),(0x395076,self.heap_alloc),(0x39506a,self.heap_free),(0x3952c6,self.get_acp),(0x3952b4,self.valid_codepage),(0x394dc4,self.cp_info),(0x394dd0,self.wake_all)]:
   self.u.mem_write(addr,b'\xc3');self.hook(addr,fn)
  for n,fn in [('FlsAlloc',self.fls_alloc),('FlsGetValue',self.fls_get),('FlsSetValue',self.fls_set),('FlsFree',self.fls_free),('AreFileApisANSI',lambda:self.ret(1)),('GetUserDefaultLocaleName',self.user_locale),('GetLocaleInfoEx',self.locale_info)]:
   addr=STUB+0x200+len(self.os_functions)*16;self.os_functions[n]=addr;self.hook(addr,fn)
  self.hook(STUB+0x100,self.global_allocate);self.hook(STUB+0x110,self.global_free)
  # Native global allocator context, matching the vtable ABI from A82B0.
  self.vt=self.allocate(24);self.ptr(self.vt+8,STUB+0x100);self.ptr(self.vt+16,STUB+0x110)
  self.allocator=self.allocate(16);self.ptr(self.allocator,self.vt);self.ptr(BASE+0x29dff0,self.allocator)
 def invalid(self,u,access,address,size,value,data):
  self.lastbad={'access':access,'address':hex(address),'size':size,'stackImage':[hex(w-BASE) for w in struct.unpack('<128Q',bytes(self.u.mem_read(self.reg(UC_X86_REG_RSP),1024))) if BASE+0x1000<=w<BASE+0x1e3000],'rip':hex(self.reg(UC_X86_REG_RIP)),'return':hex(self.q(self.reg(UC_X86_REG_RSP)))}
  return False
 def hook(self,addr,fn):
  self.hooks[addr]=fn
  def call(u,a,n,_):
   try:fn()
   except Exception as e:self.lastbad={'hook':hex(a),'type':type(e).__name__,'message':str(e),'return':hex(self.q(self.reg(UC_X86_REG_RSP))),'stackImage':[hex(w-BASE) for w in struct.unpack('<128Q',bytes(self.u.mem_read(self.reg(UC_X86_REG_RSP),1024))) if BASE+0x1000<=w<BASE+0x1e3000]};u.emu_stop()
  self.u.hook_add(UC_HOOK_CODE,call,begin=addr,end=addr)
 def reg(self,x):return self.u.reg_read(x)
 def q(self,p):return struct.unpack('<Q',self.u.mem_read(p,8))[0]
 def i(self,p):return struct.unpack('<I',self.u.mem_read(p,4))[0]
 def ptr(self,p,v):self.u.mem_write(p,struct.pack('<Q',v))
 def puti(self,p,v):self.u.mem_write(p,struct.pack('<I',v))
 def ret(self,v=0):
  sp=self.reg(UC_X86_REG_RSP);target=self.q(sp);self.u.reg_write(UC_X86_REG_RAX,v);self.u.reg_write(UC_X86_REG_RSP,sp+8);self.u.reg_write(UC_X86_REG_RIP,target)
 def owned(self,p,n,write=False):
  if n==0:return True
  if STACK<=p<=STACK+STACKSIZE and n<=STACK+STACKSIZE-p:return True
  if not write and BASE<=p<=BASE+len(self.im.data) and n<=BASE+len(self.im.data)-p:return True
  if write and any(p>=a and p-a<=size and n<=size-(p-a) for a,size in self.mutable_sections):return True
  return any(p>=a and p-a<=size and n<=size-(p-a) for a,size in self.allocs.items())
 def allocate(self,n,family="host"):
  if n<0 or n>16*1024*1024:raise ValueError(f'bounded allocation {n}')
  n=max(n,1);p=(self.top+15)&~15;end=p+n
  if end>HEAP+HEAPSIZE:raise ValueError('bounded arena full')
  self.top=(end+15)&~15;self.allocs[p]=n;self.families[p]=family;self.u.mem_write(p,bytes(n));return p
 def before_raise(self):
  sp=self.reg(UC_X86_REG_RSP);words=struct.unpack('<64Q',bytes(self.u.mem_read(sp,512)))
  report={'signal':self.reg(UC_X86_REG_RCX),'return':hex(self.q(sp)),'imageStack':[hex(w-BASE) for w in words if BASE+0x1000<=w<BASE+0x1e3000]}
  print('ORIGINAL_RAISE',json.dumps(report),flush=True)
 def virtual_protect(self):
  ptr,size,protection,old=(self.reg(x) for x in [UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8,UC_X86_REG_R9])
  if not BASE+0x1e3000<=ptr or ptr+size>BASE+len(self.im.data) or protection not in [2,4] or not self.owned(old,4,True):raise ValueError('unsupported VirtualProtect range')
  page=ptr&~4095;length=((ptr+size+4095)&~4095)-page
  self.puti(old,self.protections.get(page,4));self.protections[page]=protection
  self.u.mem_protect(page,length,UC_PROT_READ|(UC_PROT_WRITE if protection==4 else 0));self.ret(1)
 def valid_codepage(self):
  cp=self.reg(UC_X86_REG_RCX)&0xffffffff;result=int(cp in [1252,65001]);self.import_events.append({'NLS':'IsValidCodePage','codePage':cp,'return':result});self.ret(result)
 def cp_info(self):
  cp=self.reg(UC_X86_REG_RCX)&0xffffffff;out=self.reg(UC_X86_REG_RDX)
  if cp not in [0,1252,65001]:raise ValueError('unmodeled CPINFO '+str(cp))
  raw=struct.pack('<I',4 if cp==65001 else 1)+b'?\0'+bytes(14)
  if not self.owned(out,20,True):raise ValueError('unowned CPINFO output')
  self.u.mem_write(out,raw);self.import_events.append({'NLS':'GetCPInfo','codePage':cp,'return':1,'bytes':raw.hex()});self.ret(1)
 def locale_info(self):
  locale_ptr,kind,out,count=(self.reg(r) for r in [UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8,UC_X86_REG_R9]);kind&=0xffffffff;count&=0xffffffff
  locale=self.cstring(locale_ptr,True) if locale_ptr else 'en-US'
  if locale!='en-US':raise ValueError('unmodeled locale '+locale)
  values={0x1001:'English',0x1002:'United States',0x1004:'1252',1:'0409',5:'1',2:'English (United States)',6:'United States',0x5c:'en-US'}
  field=kind&0xffff
  if field not in values:raise ValueError('unmodeled LCType '+hex(kind))
  number=bool(kind&0x20000000);value=values[field]
  raw=struct.pack('<I',int(value,16 if field==1 else 10)) if number else (value+'\0').encode('utf-16le');needed=len(raw)//2
  report={'NLS':'GetLocaleInfoEx','locale':locale,'type':hex(kind),'capacity':count,'return':needed,'utf16orDword':raw.hex()}
  if count:
   if count<needed or not self.owned(out,len(raw),True):self.last_error=122;report['return']=0;self.import_events.append(report);self.ret(0);return
   self.u.mem_write(out,raw)
  self.import_events.append(report);self.ret(needed)
 def get_acp(self):
  self.import_events.append({'NLS':'GetACP','profile':'en-US','return':1252});self.ret(1252)
 def locale_request(self):
  ptr=self.reg(UC_X86_REG_RCX)
  self.import_events.append({'CRTLocaleRequest':self.cstring(ptr,True) if ptr else None,'return':hex(self.q(self.reg(UC_X86_REG_RSP)))})
 def user_locale(self):
  output=self.reg(UC_X86_REG_RCX);capacity=self.reg(UC_X86_REG_RDX)&0xffffffff
  raw='en-US\0'.encode('utf-16le');needed=len(raw)//2
  if capacity<needed or not self.owned(output,len(raw),True):self.last_error=122;self.ret(0);return
  self.u.mem_write(output,raw);self.import_events.append({'NLS':'GetUserDefaultLocaleName','profile':'en-US','capacity':capacity,'return':needed,'utf16':raw.hex()});self.ret(needed)
 def cstring(self,p,wide=False):
  data=bytes(self.u.mem_read(p,256))
  return data.decode('utf-16le').split('\0')[0] if wide else data.split(b'\0')[0].decode()
 def load_library(self):
  name=self.cstring(self.reg(UC_X86_REG_RCX),True);self.import_events.append({'LoadLibrary':name})
  if name.lower() not in ['kernel32','kernel32.dll'] and not (name.startswith('api-ms-win-core-') and (name+'\0').encode('utf-16le') in self.im.data):raise ValueError('unimplemented module '+name)
  self.ret(0x100)
 def get_proc(self):
  name=self.cstring(self.reg(UC_X86_REG_RDX));self.import_events.append({'GetProcAddress':name})
  if name=='FlsGetValue2':self.ret(0);return
  if name not in self.os_functions:
   if len(self.os_functions)>=128:raise ValueError('too many runtime resolver entries')
   addr=STUB+0x200+len(self.os_functions)*16
   def unsupported():raise ValueError('called unimplemented WinAPI '+name)
   self.os_functions[name]=addr;self.hook(addr,unsupported)
  self.ret(self.os_functions[name])
 def fls_alloc(self):
  index=len(self.fls);self.fls[index]=0;self.ret(index)
 def fls_get(self):
  index=self.reg(UC_X86_REG_RCX)
  if index not in self.fls:raise ValueError('unallocated FLS index')
  self.ret(self.fls[index])
 def fls_set(self):
  index=self.reg(UC_X86_REG_RCX)
  if index not in self.fls:raise ValueError('unallocated FLS index')
  self.fls[index]=self.reg(UC_X86_REG_RDX);self.ret(1)
 def fls_free(self):
  self.fls.pop(self.reg(UC_X86_REG_RCX));self.ret(1)
 def wake_all(self):
  ptr=self.reg(UC_X86_REG_RCX)
  if not self.owned(ptr,8,True):raise ValueError('unowned condition variable')
  # A single emulated thread has no sleeping waiter; SleepConditionVariable
  # stays unsupported, so no undisclosed synchronization success is invented.
  self.ret()
 def critical_enter(self):
  key=self.reg(UC_X86_REG_RCX)
  if not (self.owned(key,40,True) or BASE+0x1e3000<=key<=BASE+len(self.im.data)-40):raise ValueError('unowned OS critical section')
  self.critical_locks[key]=self.critical_locks.get(key,0)+1;self.ret()
 def critical_leave(self):
  key=self.reg(UC_X86_REG_RCX)
  if self.critical_locks.get(key,0)<1:raise ValueError('unbalanced critical section')
  self.critical_locks[key]-=1;self.ret()
 def srw_acquire(self):
  key=self.reg(UC_X86_REG_RCX)
  if key in self.srw_locks:raise ValueError('recursive SRW acquire')
  self.srw_locks.add(key);self.ret()
 def srw_release(self):
  key=self.reg(UC_X86_REG_RCX)
  if key not in self.srw_locks:raise ValueError('unbalanced SRW release')
  self.srw_locks.remove(key);self.ret()
 def atexit(self):
  callback=self.reg(UC_X86_REG_RCX)
  if not BASE+0x1000<=callback<BASE+0x1e3000 or len(self.exit_callbacks)>=64:raise ValueError('unexpected exit callback')
  self.exit_callbacks.append(callback);self.ret(0)
 def set_last_error(self):
  self.last_error=self.reg(UC_X86_REG_RCX)&0xffffffff;self.ret()
 def filetime(self):
  to=self.reg(UC_X86_REG_RCX)
  if not self.owned(to,8,True):raise ValueError('unowned FILETIME output')
  self.ptr(to,(time.time_ns()//100)+116444736000000000);self.ret()
 def heap_alloc(self):
  handle,flags,n=(self.reg(r) for r in [UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8])
  if handle!=0x1000 or flags not in [0,8]:raise ValueError('unsupported HeapAlloc handle/flags')
  self.ret(self.allocate(n,"heap"))
 def heap_free(self):
  handle,flags,p=(self.reg(r) for r in [UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8])
  if handle!=0x1000 or flags!=0:raise ValueError('unsupported HeapFree handle/flags')
  if p:
   if self.families.get(p)!='heap':raise ValueError('HeapFree rejects non-heap ownership')
   self.allocs.pop(p);self.families.pop(p)
  self.ret(1)
 def opnew(self):self.ret(self.allocate(self.reg(UC_X86_REG_RCX)))
 def aligned_alloc(self):self.ret(self.allocate(self.reg(UC_X86_REG_RCX)))
 def calloc(self):self.ret(self.allocate(self.reg(UC_X86_REG_RCX)*self.reg(UC_X86_REG_RDX)))
 def global_allocate(self):self.ret(self.allocate(self.reg(UC_X86_REG_RDX),"plugin"))
 def global_free(self):
  p=self.reg(UC_X86_REG_RDX)
  if p:
   if self.families.get(p)!='plugin':raise ValueError('plugin free rejects foreign family')
   self.allocs.pop(p);self.families.pop(p)
  self.ret()
 def plain_free(self):
  p=self.reg(UC_X86_REG_RCX)
  if p:self.allocs.pop(p)
  self.ret()
 def free_slot(self):
  slot=self.reg(UC_X86_REG_RCX);assert self.owned(slot,8,True);p=self.q(slot)
  if p:self.allocs.pop(p);self.ptr(slot,0)
  self.ret()
 def memcpy(self):
  to,src,n=(self.reg(x) for x in (UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8))
  if not self.owned(to,n,True) or not self.owned(src,n):raise ValueError(f'memcpy unowned {to:x} {src:x} {n}')
  self.u.mem_write(to,bytes(self.u.mem_read(src,n)));self.ret(to)
 def memset(self):
  to,byte,n=(self.reg(x) for x in (UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8))
  if not self.owned(to,n,True):raise ValueError(f'memset unowned {to:x} {n}')
  self.u.mem_write(to,bytes([byte&255])*n);self.ret(to)
 def crt_lock(self):
  key=self.reg(UC_X86_REG_RCX);self.crt_locks[key]=self.crt_locks.get(key,0)+1;self.ret()
 def crt_unlock(self):
  key=self.reg(UC_X86_REG_RCX)
  if self.crt_locks.get(key,0)<1:raise ValueError('unbalanced CRT unlock')
  self.crt_locks[key]-=1;self.ret()
 def lock(self):
  p=self.reg(UC_X86_REG_RCX);assert self.owned(p,0x50,True)
  count=self.i(p+0x4c);owner=self.i(p+0x48)
  if count and owner!=1:raise ValueError('foreign single-thread mutex')
  self.puti(p+0x48,1);self.puti(p+0x4c,count+1);self.ret(0)
 def unlock(self):
  p=self.reg(UC_X86_REG_RCX);count=self.i(p+0x4c)
  if count<1:raise ValueError('unbalanced unlock')
  self.puti(p+0x4c,count-1)
  if count==1:self.puti(p+0x48,0xffffffff)
  self.ret(0)
 def run(self,entry,this,*args):
  self.current=hex(entry);sp=STACK+STACKSIZE-0x1008;self.ptr(sp,STOP)
  self.u.reg_write(UC_X86_REG_RSP,sp);self.u.reg_write(UC_X86_REG_RCX,this)
  for reg,v in zip([UC_X86_REG_RDX,UC_X86_REG_R8,UC_X86_REG_R9],args):self.u.reg_write(reg,v)
  for i,v in enumerate(args[3:]):self.ptr(sp+0x28+8*i,v)
  self.lastbad=None;t=time.monotonic()
  try:self.u.emu_start(BASE+entry,STOP+1,timeout=30*1000000,count=200000000)
  except UcError as e:raise RuntimeError(f'{entry:x} {e} {self.lastbad}')
  rip=self.reg(UC_X86_REG_RIP)
  if rip!=STOP:raise RuntimeError(f'{entry:x} not finished rip{rip:x} bad={self.lastbad}')
  event={'entry':hex(entry),'return':self.reg(UC_X86_REG_RAX),'seconds':round(time.monotonic()-t,3),'allocated':self.top-HEAP};self.events.append(event);print(json.dumps(event),flush=True);return event['return']
