"""Independent API/SQLite/FFmpeg audit of the Rust + actual WebView cover proof."""
import hashlib
import json
from pathlib import Path
import sqlite3
import statistics
import subprocess
import sys


def main(root):
    load = lambda p: json.loads(p.read_text())
    plan=load(root/'plan.json')
    runs=[root/'first']+([root/'reopen'] if (root/'reopen').is_dir() else [])
    final=runs[-1]
    assert load(final/'runner.json')['ok'] and load(final/'complete.json')['cleanupOk']
    assert load(final/'cleanup.json')['ids']==plan['ids']
    requests=[]
    for run in runs:
        assert load(run/'runner.json')['ok']
        for path in sorted(run.glob('api-*-request.json')):
            requests.append((load(path),load(path.with_name(path.name.replace('request','response')))))
    routes=[r['path'] for r,_ in requests]
    writes=['/api/playlist/create','/api/nos/token/alloc','/api/playlist/cover/update','/api/playlist/delete']
    assert [r for r in routes if r in writes]==writes
    one=lambda route: next((r,v) for r,v in requests if r['path']==route)
    for route in writes: assert one(route)[1]['code']==200
    created,reply=one(writes[0])
    pid=str(reply.get('id') or reply['playlist']['id'])
    assert created['payload']==dict(name=plan['name'],privacy=10,uid=plan['owner'])
    assert pid not in plan['ids']
    req,reply=one(writes[1]);allocation=reply.get('data') or reply['result']
    assert req['payload']==dict(filename='cover.jpg',ext='jpg',type='image',bucket='yyimgs')
    document=str(allocation['docId'])
    assert one(writes[2])[0]['payload']==dict(id=pid,coverImgId=document)
    assert one(writes[3])[0]['payload']==dict(pid=pid)
    assert not any(r in routes for r in ['/api/playlist/update','/api/playlist/name/update','/api/playlist/tags/update','/api/playlist/desc/update'])
    before=load(runs[0]/'cover_selected.json')
    assert before['crop']==dict(x=143,y=63,size=354)
    if len(runs)==2:
        held=load(runs[0]/'complete.json');reopened=load(final/'cover_reopened.json')
        assert held['held'] and held['nativeTasks'][0]['id']==reopened['nativeTasks'][0]['id']
        assert reopened['phase']=='draft' and reopened['crop']==before['crop']
        assert held['nativeJournal'][held['nativeTasks'][0]['id']]['source']=='<retained>'
    saved=load(final/'cover_confirmed.json')
    crop=saved['intended']
    # Independent expected geometry: 362/640 horizontal-cover, zoom2,
    # keyboard -20px then pointer(+30,+10), integer original pixel rectangle.
    assert crop==dict(x=223,y=143,size=177),crop
    detail=saved['nativeDetail']
    assert detail['cover_id']==document and detail['name']==plan['name'] and detail['description']==''
    assert detail['private'] and detail['tracks']==[]
    drafts=load(final/'cover_draft_preserved.json')
    if len(runs)==1:
        assert drafts['nameDraft']=='UnsavedCoverDraft'
        assert drafts['descriptionDraft']=='Unsaved description stays local'
    cycles=[];page=[]
    for req,reply in requests:
        if req['path']!='/api/user/playlist':continue
        if int(req['payload'].get('offset',0))==0:page=[]
        page.extend(str(p['id']) for p in reply['playlist'])
        if not reply.get('more',False):cycles.append(page[:])
    assert cycles[0]==cycles[-1]==plan['ids']
    db=sqlite3.connect(f'file:{root.resolve()}/state/core.sqlite3?mode=ro',uri=True)
    def record(kind,key):return json.loads(db.execute('SELECT value FROM state WHERE owner=? AND kind=? AND key=?',(plan['owner'],kind,key)).fetchone()[0])
    task_id=saved['nativeTasks'][0]['id'];task=record('playlist-covers-v1',task_id)
    child=record('nos_uploads_v1',task_id);binding=record('playlist-mutations',task['binding_id'])
    assert task['phase']=='confirmed' and task['crop']==crop
    assert task['source']==task['encoded']=='' and not (root/'state'/f'playlist-cover-{task_id}.jpg').exists()
    assert task['allocation']['resource']==child['allocation']['resource']==document
    assert child['status']=='uploaded' and child['pending'] is None and binding['phase']=='confirmed'
    assert binding['command']==dict(kind='cover',id=pid,revision=task['revision'],document_id=document)
    jpeg=(root/'uploaded.jpg').read_bytes()
    assert hashlib.sha256(jpeg).hexdigest()==task['sha256']==child['source']['sha256']
    assert hashlib.md5(jpeg).hexdigest()==task['md5']==child['source']['md5']
    assert len(jpeg)==task['total']==child['offset']
    def rgb(path):return subprocess.run(['ffmpeg','-v','error','-i',str(path),'-frames:v','1','-f','rawvideo','-pix_fmt','rgb24','-'],check=True,capture_output=True).stdout
    source,decoded=rgb(root/'source.png'),rgb(root/'uploaded.jpg')
    assert len(source)==640*480*3 and len(decoded)==crop['size']**2*3
    expected=b''.join(source[((y+crop['y'])*640+crop['x'])*3:((y+crop['y'])*640+crop['x']+crop['size'])*3] for y in range(crop['size']))
    error=statistics.mean(abs(a-b) for a,b in zip(expected,decoded))
    assert error<3,error
    report=dict(ok=True,real_dom_ipc_picker=True,isolated_mutations=4,baseline_count=len(plan['ids']),full_order_restored=True,
                exact_document_binding=True,crop=crop,crop_mean_rgb_error=error,native_cleanup=True,
                source_unchanged=all(load(run/'runner.json')['sourceUnchanged'] for run in runs),
                restart_selection=len(runs)==2,screenshots=sum(len(list(run.glob('cover_*.png'))) for run in runs),
                remote_image_deleted=False,image_orphan_possible=True)
    (root/'independent-audit.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))

if __name__=='__main__':main(Path(sys.argv[1]))
