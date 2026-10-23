#!/usr/bin/env python3
"""Opt-in macOS acceptance against a real CLI. All write targets are disposable.

Usage: python3 tests/real_app_server.py /absolute/path/to/codex
Does not replace a deployed binary or change ChatGPT/Tunnel/Keychain configuration.
"""
import base64, concurrent.futures, hashlib, http.server, json, os, pathlib, platform, selectors, struct, subprocess, sys, tempfile, threading, time, uuid, zlib
REPO = pathlib.Path(__file__).resolve().parents[1]
BINARY = pathlib.Path(os.environ.get('LOCAL_MCP_TEST_BINARY', str(REPO/'target/debug/local-mcp'))).resolve()
CODEX = pathlib.Path(sys.argv[1]).resolve()
CHECKS = []
SOURCE_FILES=sorted((REPO/'src').glob('*.rs'))+[REPO/'Cargo.toml',REPO/'Cargo.lock']
SOURCE_AT_START={str(p.relative_to(REPO)):hashlib.sha256(p.read_bytes()).hexdigest() for p in SOURCE_FILES}
BINARY_AT_START=hashlib.sha256(BINARY.read_bytes()).hexdigest()
def check(name, value):
    assert value, name
    CHECKS.append(name)
    print('PASS', name, flush=True)
def read_json(process, timeout=40):
    deadline = time.monotonic()+timeout
    selector=selectors.DefaultSelector();selector.register(process.stdout,selectors.EVENT_READ)
    try:
        while time.monotonic()<deadline:
            if selector.select(max(0,deadline-time.monotonic())):
                line=process.stdout.readline()
                if not line: raise RuntimeError('MCP exited: '+process.stderr.read().decode())
                return json.loads(line)
        raise TimeoutError('MCP response')
    finally:selector.close()
def tool(session, name, **arguments):
    arguments['session_id']=session
    env=os.environ.copy();env['LOCAL_MCP_SESSION']=session
    p=subprocess.Popen([str(BINARY),'mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
    try:
        def send(v):p.stdin.write(json.dumps(v).encode()+b'\n');p.stdin.flush()
        send({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'local-mcp-acceptance','version':'1'}}})
        initialize=read_json(p);assert 'result' in initialize,initialize
        send({'jsonrpc':'2.0','method':'notifications/initialized'})
        send({'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':name,'arguments':arguments}})
        response=read_json(p)
        if 'error' in response:raise AssertionError(response)
        result=response['result']
        assert 'structuredContent' in result,result
        return result
    finally:
        p.stdin.close()
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=5)
def data(result):return result['structuredContent']
def wait(session,runtime,job):
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        v=data(tool(session,'command_status',runtime_id=runtime,job_id=job,wait_ms=500))
        if v.get('state') in ('finished','unknown'):return v
    raise TimeoutError('job completion: '+str(v))

def main():
    with tempfile.TemporaryDirectory(prefix='local-mcp-acceptance-') as directory:
        root=pathlib.Path(directory).resolve();project=root/'project';project.mkdir();outside=root/'outside';outside.mkdir()
        subprocess.run(['git','init','-q',str(project)],check=True)
        (project/'text.txt').write_bytes(b'before\r\n')
        subprocess.run(['git','-C',str(project),'add','text.txt'],check=True)
        index=(project/'.git/index').read_bytes()
        config=root/'settings.json';config.write_text(json.dumps({'max_jobs':2,'extra_roots':[str(outside)],'allow_git_metadata':True,'allow_network':True}))
        session='acceptance-'+uuid.uuid4().hex[:12]
        env=os.environ.copy();env['CONTROL_PLANE_API_KEY']='synthetic-should-never-reach-command'
        owner=subprocess.Popen([str(BINARY),'start',session,'--config',str(config),'--codex',str(CODEX)],cwd=project,stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,env=env)
        diagnostics=None
        try:
            deadline=time.monotonic()+45
            while time.monotonic()<deadline:
                line=owner.stderr.readline().decode()
                if not line:raise RuntimeError('start exited '+str(owner.poll()))
                if line.startswith('Session ready:'):break
            else:raise TimeoutError('start readiness')
            info=data(tool(session,'session_info'));runtime=info['runtime_id'];diagnostics=pathlib.Path(info['session']['diagnostics_directory'])
            check('App Server starts without login/model calls',info['backend_available'])
            check('concurrency ceiling reflects trusted config',info['session']['settings']['max_jobs']==2)
            duplicate=subprocess.run([str(BINARY),'start',session,'--codex',str(CODEX)],cwd=project,input=b'',stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,timeout=5)
            check('duplicate start cannot replace the session',duplicate.returncode!=0 and data(tool(session,'session_info'))['runtime_id']==runtime)

            check('Git metadata explicitly included',str(project/'.git') in info['session']['roots'])
            def run(request,argv,**kw):return data(tool(session,'exec_command',runtime_id=runtime,request_id=request,argv=argv,wait_ms=0,**kw))
            first=run('allowed',['/bin/sh','-c','printf allowed > allowed.txt; printf out; printf err >&2; exit 7'])
            done=wait(session,runtime,first['job_id'])
            check('basic root write succeeds', (project/'allowed.txt').read_text()=='allowed')
            check('nonzero exit is a result and streams remain separate',done['ok'] and done['exit_code']==7 and done['stdout']['text']=='out' and done['stderr']['text']=='err')
            again=run('allowed',['/bin/sh','-c','printf allowed > allowed.txt; printf out; printf err >&2; exit 7'])
            check('MCP reconnect preserves job and request identity',again['job_id']==first['job_id'] and again['exit_code']==7)
            mismatch=run('allowed',['/usr/bin/true']);check('same ID different content rejected',mismatch['ok'] is False)
            for request,target in [('outside',outside/'denied'),('symlink',project/'link/denied')]:
                if request=='symlink':(project/'link').symlink_to(outside,target_is_directory=True)
                v=run(request,['/bin/sh','-c','printf bad > "$1"','test',str(target)]);done=wait(session,runtime,v['job_id'])
                check(request+' write denied by real macOS sandbox',done['exit_code']!=0 and not (outside/'denied').exists())
            check('outside cwd rejected before dispatch',run('outside-cwd',['/usr/bin/true'],cwd=str(outside))['ok'] is False)
            expanded=data(tool(session,'exec_command_expanded',runtime_id=runtime,request_id='expanded',argv=['/bin/sh','-c','printf expanded > "$1"','test',str(outside/'expanded')],wait_ms=0,reason='disposable acceptance target',access={'mode':'limited','roots':[str(outside)],'network':False}))
            check('explicit extra root succeeds within fixed ceiling',wait(session,runtime,expanded['job_id'])['exit_code']==0 and (outside/'expanded').exists())
            disabled=data(tool(session,'exec_command_expanded',runtime_id=runtime,request_id='full',argv=['/usr/bin/true'],reason='test rejection',access={'mode':'full'}))
            check('full access is disabled by default',disabled['ok'] is False)
            envjob=run('env',['/bin/sh','-c','test -z "${CONTROL_PLANE_API_KEY+x}" && test -z "${CODEX_HOME+x}" && test -n "$HOME" && printf safe'])
            check('command environment excludes control key and backend home',wait(session,runtime,envjob['job_id'])['exit_code']==0)
            outside_read=data(tool(session,'read_file',runtime_id=runtime,path=str(config)))
            check('host-user read outside write roots preserved',outside_read['ok'] and 'max_jobs' in outside_read['text'])
            read=data(tool(session,'read_file',runtime_id=runtime,path='text.txt'))
            check('UTF-8 CRLF preserved',read['text']=='before\r\n')
            patch='diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+created\n'
            applied=data(tool(session,'apply_diff',runtime_id=runtime,request_id='patch',diff=patch))
            applied=wait(session,runtime,applied['job_id'])
            check('Git check then apply creates text file',applied['exit_code']==0 and (project/'new.txt').read_text()=='created\n')
            repeat=data(tool(session,'apply_diff',runtime_id=runtime,request_id='patch',diff=patch))
            check('diff redelivery does not reapply',repeat['job_id']==applied['job_id'])
            check('Git index is unchanged',(project/'.git/index').read_bytes()==index)
            badpatch='--- a/new.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-not-the-content\n+oops\n'
            failed=data(tool(session,'apply_diff',runtime_id=runtime,request_id='badpatch',diff=badpatch));failed=wait(session,runtime,failed['job_id'])
            check('failed Git check does not apply',(project/'new.txt').read_text()=='created\n' and failed['exit_code']!=0)
            metadata=run('git-metadata',['git','add','new.txt'])
            check('explicit Git metadata root permits requested index update',wait(session,runtime,metadata['job_id'])['exit_code']==0 and (project/'.git/index').read_bytes()!=index)

            closed=run('closed-stdin',['/bin/cat'])
            check('default stdin is closed',wait(session,runtime,closed['job_id'])['exit_code']==0)
            p=run('pipe',['/bin/cat'],input_mode='pipe')
            inp=dict(runtime_id=runtime,job_id=p['job_id'],request_id='stdin',data='one\n')
            check('pipe input accepted',data(tool(session,'write_stdin',**inp))['delivery']=='accepted')
            check('input redelivery uses original receipt',data(tool(session,'write_stdin',**inp))['delivery']=='accepted')
            tool(session,'write_stdin',runtime_id=runtime,job_id=p['job_id'],request_id='eof',eof=True)
            check('EOF exits and input is delivered once',wait(session,runtime,p['job_id'])['stdout']['text']=='one\n')
            running=run('stop',['/bin/sleep','60'])
            stop=data(tool(session,'stop_command',runtime_id=runtime,job_id=running['job_id']))
            check('stop ACK observed on original connection',stop['stop_ack']['delivery']=='accepted')
            check('stop waits for final exec result',wait(session,runtime,running['job_id'])['state']=='finished')
            pty=run('pty',['/bin/sh','-c','test -t 0 && stty size; cat'],input_mode='pty',size={'rows':20,'cols':80})
            state=data(tool(session,'command_status',runtime_id=runtime,job_id=pty['job_id'],wait_ms=500))
            check('PTY output is explicitly multiplexed',state['pty_multiplexed'] and '20 80' in state['stdout']['text'])
            resized=data(tool(session,'write_stdin',runtime_id=runtime,job_id=pty['job_id'],request_id='resize',resize={'rows':30,'cols':100}))
            check('PTY resize accepted',resized['delivery']=='accepted')
            second=run('full-second',['/bin/sleep','60'])
            check('job ceiling rejects a third start',run('over-capacity',['/usr/bin/true'])['ok'] is False)
            check('status progresses at full job capacity',data(tool(session,'command_status',runtime_id=runtime,job_id=second['job_id']))['state']=='running')
            for job in [pty,second]:
                tool(session,'stop_command',runtime_id=runtime,job_id=job['job_id']);wait(session,runtime,job['job_id'])
            class Handler(http.server.BaseHTTPRequestHandler):
                def do_GET(self):
                    self.send_response(200);self.end_headers();self.wfile.write(b'local-test')
                def log_message(self,*_):pass
            httpd=http.server.HTTPServer(('127.0.0.1',0),Handler)
            thread=threading.Thread(target=httpd.serve_forever,daemon=True);thread.start()
            try:
                url='http://127.0.0.1:'+str(httpd.server_port)
                denied=run('network-denied',['/usr/bin/curl','--max-time','2','--silent',url])
                check('ordinary execution denies loopback network',wait(session,runtime,denied['job_id'])['exit_code']!=0)
                network=data(tool(session,'exec_command_expanded',runtime_id=runtime,request_id='network-allowed',argv=['/usr/bin/curl','--max-time','2','--silent',url],wait_ms=0,reason='disposable loopback acceptance target',access={'mode':'limited','roots':[],'network':True}))
                check('explicit network expansion works within ceiling',wait(session,runtime,network['job_id'])['stdout']['text']=='local-test')
            finally:httpd.shutdown();httpd.server_close();thread.join()
            # A self-created image is returned as native content; this does not prove ordinary Chat vision.
            png=project/'pixel.png'
            def png_chunk(kind,payload):return struct.pack('>I',len(payload))+kind+payload+struct.pack('>I',zlib.crc32(kind+payload)&0xffffffff)
            png.write_bytes(b'\x89PNG\r\n\x1a\n'+png_chunk(b'IHDR',struct.pack('>IIBBBBB',2,2,8,2,0,0,0))+png_chunk(b'IDAT',zlib.compress(b'\0\xff\0\0\0\xff\0\0\0\0\xff\xff\xff\xff'))+png_chunk(b'IEND',b''))
            formats=[('png',png),('jpeg',project/'pixel.jpeg'),('gif',project/'pixel.gif'),('bmp',project/'pixel.bmp'),('tiff',project/'pixel.tiff'),('avif',project/'pixel.avif')]
            for fmt,path in formats:
                if fmt!='png':subprocess.run(['/usr/bin/sips','-s','format',fmt,str(png),'--out',str(path)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                image=tool(session,'view_image',runtime_id=runtime,path=str(path))
                check('native MCP image '+fmt,image.get('isError') is not True and any(c['type']=='image' for c in image['content']))
            webp=project/'pixel.webp';webp.write_bytes(base64.b64decode('UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA'))
            image=tool(session,'view_image',runtime_id=runtime,path=str(webp))
            check('native MCP image webp',image.get('isError') is not True and any(c['type']=='image' for c in image['content']))
            large_text=project/'large.txt';large_text.write_bytes((b'x'*1022+b'\r\n')*12288)
            page=data(tool(session,'read_file',runtime_id=runtime,path=str(large_text)))
            check('12 MiB source text has explicit bounded continuation',page['has_more'] and page['next_line']==page['lines_returned']+1 and page['text'].endswith('\r\n'))
            following=data(tool(session,'read_file',runtime_id=runtime,path=str(large_text),start_line=page['next_line'],max_lines=1))
            check('line-range read has no source-file size cap',following['text']=='x'*1022+'\r\n')
            width,height=1900,2000;row=((width*3+3)//4)*4;pixels=b'\x00\x80\xff'*(width)+b'\x00'*(row-width*3)
            bmp=project/'large.bmp';bmp.write_bytes(b'BM'+struct.pack('<IHHI',54+row*height,0,0,54)+struct.pack('<IiiHHIIiiII',40,width,height,1,24,0,row*height,2835,2835,0,0)+pixels*height)
            image=tool(session,'view_image',runtime_id=runtime,path=str(bmp))
            check('11 MiB image survives base64 JSON stdio delivery',image.get('isError') is not True and data(image)['bytes']==bmp.stat().st_size and len(json.dumps(image))<16*1024*1024)
            invalid=project/'invalid.png';invalid.write_bytes(b'\x89PNG\r\n\x1a\ninvalid')
            check('invalid image is rejected',data(tool(session,'view_image',runtime_id=runtime,path=str(invalid)))['ok'] is False)
            peaks=[]
            backend_ids=subprocess.check_output(['/usr/bin/pgrep','-P',str(owner.pid)],text=True).split()
            for mib in [64,256]:
                sample=True;measure=[]
                def sample_rss():
                    while sample:
                        rss=subprocess.check_output(['/bin/ps','-o','rss=','-p',','.join([str(owner.pid)]+backend_ids)],text=True)
                        measure.append([int(n) for n in rss.split()]);time.sleep(.05)
                monitor=threading.Thread(target=sample_rss);monitor.start()
                try:
                    flood=run('large-'+str(mib),[sys.executable,'-c','import os; block=b"x"*65536; [os.write(1,block) for _ in range('+str(mib*16)+')]'])
                    flood=wait(session,runtime,flood['job_id'])
                    check(str(mib)+' MiB output completes without source truncation',flood['exit_code']==0 and flood['stdout']['source_end_byte']==mib*1024*1024 and not flood['stdout']['source_truncated'] and flood['stdout']['gap'])
                finally:sample=False;monitor.join()
                peaks.append({'source_mib':mib,'peak_rss_kib_by_pid':{str(pid):max((r[i] for r in measure if len(r)>i),default=0) for i,pid in enumerate([owner.pid]+[int(n) for n in backend_ids])}})
            print(json.dumps({'output_memory_samples':peaks}),flush=True)
            started=time.monotonic()
            foreground=data(tool(session,'exec_command',runtime_id=runtime,request_id='default-foreground',argv=['/bin/sleep','32']))
            elapsed=time.monotonic()-started
            check('default foreground wait remains 30 seconds',foreground['state']=='running' and 29<=elapsed<=32)
            check('no automatic backend execution deadline',wait(session,runtime,foreground['job_id'])['exit_code']==0)
            wrong=data(tool(session,'command_status',runtime_id='old',job_id=running['job_id']))
            check('old runtime rejected',wrong['ok'] is False)
            long=run('alive-after-facade-exit',['/bin/sleep','1'])
            check('job survives MCP facade exit',wait(session,runtime,long['job_id'])['exit_code']==0)
        finally:
            owner.stdin.close()
            try:owner.wait(timeout=20)
            except subprocess.TimeoutExpired:owner.terminate();owner.wait(timeout=5)
            remaining=owner.stderr.read().decode()
            if owner.returncode:print(remaining,file=sys.stderr)
        check('owned diagnostics deleted after confirmed shutdown',diagnostics is not None and not diagnostics.exists())
        check('session owner exited cleanly',owner.returncode==0)
        full_session='acceptance-full-'+uuid.uuid4().hex[:10]
        subprocess.run(['git','-C',str(project),'-c','user.name=Acceptance Fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture'],check=True)
        linked=root/'linked';subprocess.run(['git','-C',str(project),'worktree','add','-qb','fixture-linked',str(linked)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        full_config=root/'full-settings.json';full_config.write_text(json.dumps({'allow_full_access':True,'allow_git_metadata':True}))
        full_owner=subprocess.Popen([str(BINARY),'start',full_session,'--config',str(full_config),'--codex',str(CODEX)],cwd=linked,stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        full_diagnostics=None
        try:
            while True:
                line=full_owner.stderr.readline().decode()
                if not line:raise RuntimeError('full-mode start failed')
                if line.startswith('Session ready:'):break
            full_info=data(tool(full_session,'session_info'));full_runtime=full_info['runtime_id'];full_diagnostics=pathlib.Path(full_info['session']['diagnostics_directory'])
            check('linked worktree resolves both metadata locations',len(full_info['session']['git_metadata_candidates'])==2)
            (linked/'linked.txt').write_text('fixture\n')
            linked_job=data(tool(full_session,'exec_command',runtime_id=full_runtime,request_id='linked-index',argv=['git','add','linked.txt'],wait_ms=0))
            check('approved linked worktree metadata permits Git index write',wait(full_session,full_runtime,linked_job['job_id'])['exit_code']==0)

            full_job=data(tool(full_session,'exec_command_expanded',runtime_id=full_runtime,request_id='full-allowed',argv=['/bin/sh','-c','printf full > "$1"','test',str(outside/'full-allowed')],wait_ms=0,reason='explicit disposable full-access acceptance target',access={'mode':'full'}))
            full_done=wait(full_session,full_runtime,full_job['job_id'])
            check('explicitly enabled full mode writes disposable outside target',full_done['exit_code']==0 and (outside/'full-allowed').read_text()=='full')
            check('full mode reports both restrictions removed',full_done['effective_access']['mode']=='full' and full_done['effective_access']['roots'] is None and full_done['effective_access']['network'] is True)
            refused=data(tool(full_session,'exec_command_expanded',runtime_id=full_runtime,request_id='network-off',argv=['/usr/bin/true'],reason='test fixed ceiling',access={'mode':'limited','roots':[],'network':True}))
            check('limited network cannot exceed fixed startup ceiling',refused['ok'] is False)
            refused=data(tool(full_session,'exec_command_expanded',runtime_id=full_runtime,request_id='mixed-full',argv=['/usr/bin/true'],reason='test exclusive mode',access={'mode':'full','roots':[]}))
            check('full mode cannot pretend to retain limited roots',refused['ok'] is False)
        finally:
            full_owner.stdin.close();full_owner.wait(timeout=20)
        check('full-mode owner cleans only its confirmed owned storage',full_owner.returncode==0 and full_diagnostics is not None and not full_diagnostics.exists())

        report={'checks_passed':len(CHECKS),'checks':CHECKS,'codex_version':subprocess.check_output([str(CODEX),'--version'],text=True).strip(),
                'codex_executable_sha256':hashlib.sha256(CODEX.read_bytes()).hexdigest(),'macos':platform.mac_ver()[0],
                'ordinary_chat_visual_acceptance':'not_run','memory_samples':peaks,
                'adapter_executable_sha256':hashlib.sha256(BINARY.read_bytes()).hexdigest(),
                'source_sha256':{str(p.relative_to(REPO)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((REPO/'src').glob('*.rs'))+[REPO/'Cargo.toml',REPO/'Cargo.lock']}}
        assert report['source_sha256']==SOURCE_AT_START, 'source changed during acceptance'
        assert report['adapter_executable_sha256']==BINARY_AT_START, 'binary changed during acceptance'
        if len(sys.argv)>2:pathlib.Path(sys.argv[2]).write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({'checks_passed':report['checks_passed'],'codex_version':report['codex_version'],'ordinary_chat_visual_acceptance':'not_run'}))
if __name__=='__main__':main()
