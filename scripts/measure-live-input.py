"""Read-only local measurements: cursor, foreground process, agent CPU and HTTPS RTT. No keys or screenshots."""
import ctypes, ctypes.wintypes as w, json, time, os, sys, http.client, datetime
from pathlib import Path
pid=int(sys.argv[1]); duration=float(sys.argv[2]) if len(sys.argv)>2 else 120
out=Path('diagnostics')/('input-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'));out.mkdir(parents=True,exist_ok=True)
k=ctypes.windll.kernel32;u=ctypes.windll.user32
k.OpenProcess.restype=w.HANDLE;k.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD]
k.GetProcessTimes.argtypes=[w.HANDLE,ctypes.POINTER(w.FILETIME),ctypes.POINTER(w.FILETIME),ctypes.POINTER(w.FILETIME),ctypes.POINTER(w.FILETIME)]
u.GetForegroundWindow.restype=w.HWND;u.GetWindowThreadProcessId.argtypes=[w.HWND,ctypes.POINTER(w.DWORD)]
handle=k.OpenProcess(0x1000,False,pid)
def cpu():
 a,b,c,d=w.FILETIME(),w.FILETIME(),w.FILETIME(),w.FILETIME()
 if not k.GetProcessTimes(handle,ctypes.byref(a),ctypes.byref(b),ctypes.byref(c),ctypes.byref(d)):return None
 return ((c.dwHighDateTime<<32)+c.dwLowDateTime+(d.dwHighDateTime<<32)+d.dwLowDateTime)/1e7
host='vision-pro.34-73-186-88.sslip.io';rtts=[]
try:
 conn=http.client.HTTPSConnection(host,timeout=5)
 for i in range(6):
  t=time.perf_counter();conn.request('GET','/healthz');r=conn.getresponse();body=r.read();rtts.append({'ms':round((time.perf_counter()-t)*1000,2),'status':r.status,'warm':i>0})
 conn.close()
except Exception as e:rtts.append({'error':str(e)})
(out/'metadata.json').write_text(json.dumps({'pid':pid,'duration_s':duration,'https_rtt':rtts,'note':'HTTPS RTT is PC-to-relay only; cursor position is not an input latency measurement.'},indent=2))
print('RECORDING '+str(out.resolve()),flush=True)
start=time.perf_counter();samples=0
with (out/'samples.jsonl').open('w') as f:
 while time.perf_counter()-start<duration:
  t=time.perf_counter();point=w.POINT();u.GetCursorPos(ctypes.byref(point));fg=w.DWORD();u.GetWindowThreadProcessId(u.GetForegroundWindow(),ctypes.byref(fg))
  f.write(json.dumps({'elapsed_s':round(t-start,4),'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'cursor_x':point.x,'cursor_y':point.y,'foreground_pid':fg.value,'agent_cpu_s':cpu()})+'\n');f.flush();samples+=1
  time.sleep(max(0,.05-(time.perf_counter()-t)))
k.CloseHandle.argtypes=[w.HANDLE];k.CloseHandle(handle)
print(json.dumps({'folder':str(out.resolve()),'samples':samples,'https_rtt':rtts}),flush=True)
