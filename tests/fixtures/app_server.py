#!/usr/bin/python3
"""Protocol fault fixture. It deliberately supplies no filesystem sandbox."""
import sys, json, threading, base64, os, time, pathlib, subprocess, signal

# Only the capability gate's required fields; this fixture is not an API schema oracle.
if sys.argv[1:3] == ['app-server', 'generate-json-schema']:
    directory = pathlib.Path(sys.argv[sys.argv.index('--out') + 1])
    (directory/'v2').mkdir(parents=True)
    fields = {
        'CommandExecParams': ['disableTimeout','disableOutputCap','streamStdoutStderr','streamStdin','env','processId','sandboxPolicy','tty','size'],
        'CommandExecWriteParams': ['deltaBase64','closeStdin','processId'],
        'CommandExecResizeParams': ['size','processId'],
        'CommandExecTerminateParams': ['processId'],
        'CommandExecOutputDeltaNotification': ['processId','stream','deltaBase64','capReached'],
    }
    for name, keys in fields.items():
        (directory/'v2'/(name+'.json')).write_text(json.dumps({'properties': {key:{} for key in keys}}))
    (directory/'ClientRequest.json').write_text(json.dumps(['initialize','command/exec','command/exec/write','command/exec/resize','command/exec/terminate']))
    (directory/'ServerNotification.json').write_text(json.dumps(['command/exec/outputDelta']))
    sys.exit(0)
lock = threading.Lock()
process_lock = threading.Lock()
closing = False
processes = {}
def emit(value):
    with lock:
        print(json.dumps(value), flush=True)
def output(pid, data, stream='stdout', cap=False):
    emit({'method':'command/exec/outputDelta','params':{'processId':pid,'stream':stream,'deltaBase64':base64.b64encode(data).decode(),'capReached':cap}})
def finish(pid, code):
    request = processes[pid]
    emit({'id':request['id'],'result':{'exitCode':code,'stdout':'','stderr':''}})
def run(request):
    p = request['params']; pid = p['processId']; mode = p['command'][0]
    assert p['disableTimeout'] and p['disableOutputCap'] and p['streamStdoutStderr']
    if mode == 'exit':
        output(pid, b'hello\xff\r\n'); finish(pid, int(p['command'][1]))
    elif mode == 'buffered':
        emit({'id':request['id'],'result':{'exitCode':0,'stdout':'unexpected buffered data','stderr':''}})
    elif mode == 'flood':
        for _ in range(16): output(pid, b'x'*1024)
        finish(pid, 0)
    elif mode == 'unicode':
        data = ('あ😀\r\n'*40).encode()
        output(pid, data[:2]); time.sleep(.03); output(pid, data[2:])
        output(pid, '終わり\n'.encode(), 'stderr'); finish(pid, 0)
    elif mode == '/bin/sh' and 'exec git -c core.hooksPath=' in p['command'][2]:
        # Exercise the adapter's actual fixed Git scripts in disposable test
        # directories. This fixture still supplies no filesystem sandbox.
        applying = '--check' not in p['command'][2]
        if not applying and pathlib.Path(p['cwd'], 'pause-check').exists():
            output(pid, b'paused check before apply\n', 'stderr'); return
        if applying and pathlib.Path(p['cwd'], 'lose-apply').exists(): os._exit(0)
        if applying and pathlib.Path(p['cwd'], 'fail-apply').exists(): finish(pid, 1); return
        with process_lock:
            if closing: return
            child = subprocess.Popen(p['command'], cwd=p['cwd'], env=p['env'], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            request['child'] = child
        def drain(stream, name):
            while True:
                data = os.read(stream.fileno(), 4096)
                if not data: break
                output(pid, data, name)
        threads = [threading.Thread(target=drain, args=(child.stdout,'stdout')),
                   threading.Thread(target=drain, args=(child.stderr,'stderr'))]
        for thread in threads: thread.start()
        code = child.wait()
        for thread in threads: thread.join()
        finish(pid, code)
    elif mode == 'lose':
        time.sleep(.03); os._exit(0)
    elif mode == 'bad-output-then-lose':
        output(pid, b'retained\n')
        params = {'processId':pid, 'stream':'stdout', 'deltaBase64':'!invalid!'}
        if p['command'][1] == 'missing': del params['deltaBase64']
        if p['command'][1] == 'stream': params.update(stream='invalid', deltaBase64='eA==')
        emit({'method':'command/exec/outputDelta', 'params':params})
        time.sleep(.03); os._exit(0)
    else:
        output(pid, b'ready\n')
for line in sys.stdin:
    q = json.loads(line); method = q.get('method'); p = q.get('params',{})
    if method == 'initialize': emit({'id':q['id'],'result':{'userAgent':'fixture'}})
    elif method == 'command/exec':
        processes[p['processId']] = q
        threading.Thread(target=run,args=(q,),daemon=True).start()
    elif method == 'command/exec/write':
        original = processes[p['processId']]
        data = base64.b64decode(p.get('deltaBase64') or '')
        child = original.get('child')
        if child:
            child.stdin.write(data); child.stdin.flush()
            if p.get('closeStdin'): child.stdin.close()
        else: output(p['processId'], data)
        if original['params']['command'][0] != 'noack': emit({'id':q['id'],'result':{}})
        if p.get('closeStdin') and not child: finish(p['processId'], 0)
    elif method == 'command/exec/resize': emit({'id':q['id'],'result':{}})
    elif method == 'command/exec/terminate':
        emit({'id':q['id'],'result':{}})
        pid = p['processId']
        child = processes[pid].get('child')
        if child:
            if child.poll() is None:
                try: os.killpg(child.pid, signal.SIGTERM)
                except ProcessLookupError: pass
            continue
        def terminated(pid=pid):
            time.sleep(.1); finish(pid, 143)
        threading.Thread(target=terminated,daemon=True).start()
with process_lock:
    closing = True
    children = [request.get('child') for request in processes.values()]
for child in children:
    if child and child.poll() is None:
        try: os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError: pass
        child.wait()
