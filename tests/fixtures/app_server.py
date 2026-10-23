#!/usr/bin/python3
"""Protocol fault fixture. It deliberately supplies no filesystem sandbox."""
import sys, json, threading, base64, os, time
lock = threading.Lock()
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
    elif mode == 'lose':
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
        output(p['processId'], data)
        if original['params']['command'][0] != 'noack': emit({'id':q['id'],'result':{}})
        if p.get('closeStdin'): finish(p['processId'], 0)
    elif method == 'command/exec/resize': emit({'id':q['id'],'result':{}})
    elif method == 'command/exec/terminate':
        emit({'id':q['id'],'result':{}})
        pid = p['processId']
        def terminated(pid=pid):
            time.sleep(.1); finish(pid, 143)
        threading.Thread(target=terminated,daemon=True).start()
