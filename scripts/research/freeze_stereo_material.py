#!/usr/bin/env python3
"""Opt-in manual original SE1 -> default limiter subgraph oracle.

Requires the hash-pinned locally extracted DLL and Linux x86_64 namespaces.
Not public CI, not a production dependency, and not the full 1AA50/SDK graph.
"""
import argparse,gzip,hashlib,json,math,pathlib,platform,struct,subprocess,tempfile
WORK=None
FLAGS=['-std=c11','-O2','-Wall','-Wextra','-Werror','-ffp-contract=off','-fno-fast-math']
SHA='c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be'
def digest(x):return hashlib.sha256(x).hexdigest()
def pack_f(x):return struct.pack('<%df'%len(x),*x)
def bits(x):return list(struct.unpack('<%dI'%(len(x)//4),x))
def run(binary,image,wire):
    p=subprocess.run(['unshare','-Urn',str(binary),*(['process'] if binary.name=='se-original' else []),str(image)],input=wire,capture_output=True,timeout=10)
    if p.returncode:raise RuntimeError(f'{binary.name} exit={p.returncode}: {p.stderr.decode(errors="replace")[:400]}')
    return p.stdout

def execute(rate,cap,reset):
    # Official adjustable SE values; no hosted asset/real account is involved.
    controls={'presence':3.0,'stereoizer':5.0,'sshaper':True,'ambience':1}
    counts=[cap,7,0,cap,cap-1,3]
    reset_at=3 if reset else None
    source=[];cursor=0
    for i,n in enumerate(counts):
        pcm=[]
        for ch in range(2):
            pcm.extend([(.08*math.sin((cursor+j)*(.023+ch*.009))+.025*math.cos((cursor+j)*.061)) if j<n else 0.0 for j in range(cap)])
        if i==0:pcm[0]=.7;pcm[cap]=-.4
        source.append({'frames':n,'end':i==len(counts)-1,'reset':i==reset_at,'inputBits':bits(pack_f(pcm))})
        cursor+=n
    # SE receives its original zero-EOF drain blocks. The limiter's MAX_BLOCKS
    # stays untouched (256); these three rates/capacities fit that exact bound.
    for i in range(math.ceil(rate/cap)+4):
        source.append({'frames':0,'end':True,'reset':False,'inputBits':[0]*(cap*2)})
    assert len(source)+4<=256
    wire=bytearray(struct.pack('<4I',rate,cap,len(source),0))
    for row in source:
        wire.extend(struct.pack('<IIIffIiI',row['frames'],0,row['end'],3.,5.,1,1,row['reset']))
        wire.extend(struct.pack('<%dI'%(cap*2),*row['inputBits']))
    se_raw=run(WORK/'se-original',WORK/'image.bin',wire)
    size=8+4*(cap*2+22+87+70)
    assert len(se_raw)==size*len(source)
    se=[]
    for i in range(len(source)):
        at=i*size;n,end=struct.unpack_from('<II',se_raw,at);assert n<=cap and end in (0,1)
        raw=se_raw[at+8:at+8+cap*8]
        values=struct.unpack('<%df'%(cap*2),raw)
        assert all(math.isfinite(x) for x in values)
        # Unused padding is irrelevant to B0's n-bounded read, but its research
        # host validates the whole packet. Canonicalize only invalid padding.
        canonical=values[:n]+(0.,)*(cap-n)+values[cap:cap+n]+(0.,)*(cap-n)
        assert all(abs(x)<=4 for x in canonical)
        se.append({'frames':n,'end':bool(end),'reset':source[i]['reset'],'inputIndex':i,
                   'validPcmBits':bits(raw[:n*4]+raw[cap*4:cap*4+n*4]),
                   'fullPcmBits':bits(pack_f(canonical)),
                   'stateSha256':digest(se_raw[at+8+cap*8:at+size])})
    se_finish=next(i for i,r in enumerate(se) if r['end'])
    assert all(r['frames']==0 and r['end'] for r in se[se_finish+1:])
    se=se[:se_finish+1]
    lim_input=[{'frames':r['frames'],'end':r['end'],'reset':r['reset'],'pcm':r['fullPcmBits']} for r in se]
    lim_input.extend([{'frames':0,'end':True,'reset':False,'pcm':[0]*(cap*2)} for _ in range(4)])
    assert len(lim_input)<=256
    lim_wire=bytearray(struct.pack('<4I',rate,2,cap,len(lim_input)))
    for r in lim_input:
        lim_wire.extend(struct.pack('<3I',r['frames'],r['end'],3 if r['reset'] else 0))
        lim_wire.extend(struct.pack('<%dI'%(cap*2),*r['pcm']))
    lim_raw=run(WORK/'limiter-original',WORK/'image.bin',lim_wire)
    lim_size=8+cap*8;assert len(lim_raw)==lim_size*len(lim_input)
    limiter=[]
    for i in range(len(lim_input)):
        at=i*lim_size;n,end=struct.unpack_from('<II',lim_raw,at);assert n<=cap and end in (0,1)
        raw=lim_raw[at+8:at+lim_size]
        values=struct.unpack('<%df'%(cap*2),raw)
        assert all(math.isfinite(x) for x in values[:n]+values[cap:cap+n])
        limiter.append({'inputFrames':lim_input[i]['frames'],'inputEnd':lim_input[i]['end'],
                        'reset':lim_input[i]['reset'],'frames':n,'end':bool(end),
                        'validPcmBits':bits(raw[:n*4]+raw[cap*4:cap*4+n*4])})
    finish=next(i for i,r in enumerate(limiter) if r['end'])
    assert finish>=se_finish
    assert all(r['frames']==0 and r['end'] for r in limiter[finish+1:])
    se_total=sum(r['frames'] for r in se)
    out_total=sum(r['frames'] for r in limiter)
    delay={44100:219,48000:238,96000:478}[rate]
    assert limiter[0]['frames']==max(0,cap-delay)
    assert sum(r['frames'] for r in se if r['reset'])==(cap if reset else 0)
    expected_drop=min(sum(r['frames'] for r in se[:reset_at]),delay) if reset else 0
    assert out_total==se_total-expected_drop,(rate,cap,reset,se_total,out_total,expected_drop)
    return {'name':f'{rate}-{cap}-'+('reset' if reset else 'continuous'),'rate':rate,'capacity':cap,
            'channels':2,'controls':controls,'callerFtz':0,'source':source[:se_finish+1],
            'se':se,'limiter':limiter,'seFirstEndBlock':se_finish,'limiterFirstEndBlock':finish,
            'seFrames':se_total,'outputFrames':out_total,'resetDiscardedLookaheadFrames':expected_drop,
            'sourceWireSha256':digest(wire),'seRawSha256':digest(se_raw),'limiterWireSha256':digest(lim_wire),'limiterRawSha256':digest(lim_raw)}

def mapped_image(raw):
    if digest(raw)!=SHA:raise ValueError('unexpected official DLL hash')
    pe=struct.unpack_from('<I',raw,0x3c)[0]
    if raw[pe:pe+4]!=b'PE\0\0':raise ValueError('invalid PE header')
    machine,count=struct.unpack_from('<HH',raw,pe+4)
    opt_len=struct.unpack_from('<H',raw,pe+20)[0];opt=pe+24
    if machine!=0x8664 or struct.unpack_from('<H',raw,opt)[0]!=0x20b or struct.unpack_from('<Q',raw,opt+24)[0]!=0x180000000:
        raise ValueError('unexpected x64 image layout')
    size,headers=struct.unpack_from('<II',raw,opt+56)
    if size!=0x3a3000 or headers>len(raw):raise ValueError('unexpected image size')
    image=bytearray(size);image[:headers]=raw[:headers]
    for i in range(count):
        _,rva,n,offset=struct.unpack_from('<4I',raw,opt+opt_len+i*40+8)
        if offset+n>len(raw) or rva+n>size:raise ValueError('invalid section')
        image[rva:rva+n]=raw[offset:offset+n]
    return image[:0x3a2400]

def compact(case):
    original_count=len(case['source'])
    selected={0,len(case['limiter'])//2,case['seFirstEndBlock'],case['limiterFirstEndBlock']}
    blocks=[]
    for i,output in enumerate(case['limiter']):
        extra=i>=original_count
        source=case['source'][i] if not extra else {'frames':0,'end':True,'reset':False,'inputBits':[0]*(case['capacity']*2)}
        packed=struct.pack('<%dI'%len(output['validPcmBits']),*output['validPcmBits'])
        row={**source,'outputFrames':output['frames'],'outputEnd':output['end'],
             'outputSha256':digest(packed),'limiterInputFrames':output['inputFrames'],
             'limiterInputEnd':output['inputEnd']}
        if extra:row.update(sourceFrames=0,sourceEnd=True,postSeEndDrain=True)
        if i in selected:row['outputBits']=output['validPcmBits']
        blocks.append(row)
    return {key:case[key] for key in ('name','rate','capacity','channels','controls','callerFtz','seFirstEndBlock','limiterFirstEndBlock','seFrames','outputFrames','resetDiscardedLookaheadFrames')}|{'blocks':blocks}

def main():
    global WORK
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dll',type=pathlib.Path,required=True)
    parser.add_argument('--output',type=pathlib.Path,required=True)
    parser.add_argument('--se-source',type=pathlib.Path,default=pathlib.Path(__file__).with_name('stereo_one_native.c'))
    parser.add_argument('--limiter-source',type=pathlib.Path,default=pathlib.Path(__file__).with_name('native_limiter_oracle.c'))
    args=parser.parse_args()
    if platform.system()!='Linux' or platform.machine()!='x86_64':raise ValueError('requires Linux x86_64')
    manifest=args.output.with_suffix('').with_suffix('.manifest.json')
    if args.output.exists() or manifest.exists():raise FileExistsError('output and manifest must be new')
    image=mapped_image(args.dll.read_bytes())
    sources={'se':args.se_source,'limiter':args.limiter_source}
    source_hashes={name:digest(source.read_bytes()) for name,source in sources.items()}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='cumulus-stereo-material-',dir=args.output.parent) as tmp:
        WORK=pathlib.Path(tmp);(WORK/'image.bin').write_bytes(image)
        for name,source in sources.items():
            subprocess.run(['cc',*FLAGS,str(source),'-o',str(WORK/(name+'-original')),'-pthread'],check=True,timeout=30)
        full=[execute(rate,cap,reset) for rate in (44100,48000,96000) for cap in (512,2048) for reset in (False,True)]
    corpus={'schema':'cumulus-local-stereo-subgraph-v1','fullSdkGraph':False,'graphDriver1AA50Executed':False,'outerFifoExecuted':False,'cases':[compact(case) for case in full]}
    encoded=json.dumps(corpus,separators=(',',':'),ensure_ascii=True).encode()+b'\n'
    fixture=gzip.compress(encoded,mtime=0)
    meta={'schema':corpus['schema'],'dllSha256':SHA,'fixtureSha256':digest(fixture),
          'generatorSha256':digest(pathlib.Path(__file__).read_bytes()),'nativeSourceSha256':source_hashes,
          'executedSe':['B2030','B1780','B1AE0','B34C0','B3990','A85B0','A8550','A80D0'],
          'executedLimiter':['63900','63EB0','654B0','654A0','65490','63B90','63AC0'],
          'sourceReferences':['1AEC0','1BBA0','1C590','1D360','1D410','1AA50'],
          'fullSdkGraph':False,'graphDriver1AA50Executed':False,'outerFifoExecuted':False,
          'caseCount':len(full),'blockCount':sum(len(c['limiter']) for c in full),
          'seCallsThroughFirstEnd':sum(len(c['se']) for c in full),
          'seCallsActuallyExecuted':sum(6+math.ceil(c['rate']/c['capacity'])+4 for c in full),'outputFrames':sum(c['outputFrames'] for c in full),
          'rates':[44100,48000,96000],'capacities':[512,2048],'channels':2,'callerFtz':0,
          'controls':{'presence':3,'stereoizer':5,'sshaper':True,'ambience':1},
          'limiterDefaults':{'threshold':0,'attackF32':0.005,'releaseF32':0.1,'outputGain':0,'infiniteRatio':True,'ratio':4,'flag48':True},
          'inputPacking':'full capacity planar f32 little-endian bits; frames/end/reset are source inputs',
          'outputPacking':'valid L then valid R f32 little-endian bytes per block; every block has outputSha256',
          'selectedOutputBits':'first, midpoint, SE first end and B0 first end; not a full PCM public duplicate',
          'composition':'two independent namespace child processes; SE valid n/end/PCM fed unchanged to B0, then four zero/end=true calls; only invalid padding is zeroed',
          'postSeEndDrain':'extra B0 rows have sourceFrames=0/sourceEnd=true and inputBits all zero; original SE host separately proves post-end remains zero/end=true',
          'reset':'same logical block calls original SE B3990 and B0 65490; discarded previous B0 lookahead accounted',
          'adapters':{'se':['PE mapping','bounded allocator/free and block context','TEB lower stack bound'],
                      'limiter':['PE mapping','4 bounded ownership-checked CRT memory seams unchanged']},
          'excludes':['full SDK factories/initialization, master and off sibling first-block transitions','1AA50 execution, format converters and outer FIFO','DllMain/imports/Windows CRT CPU init','physical output, real assets/accounts/network','other rates/capacities/controls/FTZ inputs'],
          'compilerFlags':FLAGS,'compiler':subprocess.check_output(['cc','--version'],text=True).splitlines()[0]}
    args.output.write_bytes(fixture);manifest.write_text(json.dumps(meta,indent=2)+'\n')
    print(json.dumps({key:meta[key] for key in ('caseCount','blockCount','seCallsThroughFirstEnd','seCallsActuallyExecuted','outputFrames','fixtureSha256','fullSdkGraph')}))
if __name__=='__main__':main()
