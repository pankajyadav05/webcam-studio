"""CPU-time deltas for the launched bundle and its newly started WK helpers."""
import subprocess,time,json,pathlib,sys
mode=sys.argv[1]
rows=[line.split(None,5) for line in subprocess.check_output(['ps','-axo','pid=,ppid=,%cpu=,rss=,etime=,comm='],text=True).splitlines()]
main=[r for r in rows if '/target/release/bundle/macos/Webcam Studio.app/Contents/MacOS/webcam-studio-desktop' in r[5]][0]
def elapsed(text):
 days,_,clock=text.partition('-')
 if not clock:clock=days;days=0
 return int(days)*86400+sum(int(v)*60**i for i,v in enumerate(clock.split(':')[::-1]))
started=elapsed(main[4]);main_pid=int(main[0])
selected=[main]+[r for r in rows if 'WebKit.' in r[5] and int(r[0])>main_pid and abs(elapsed(r[4])-started)<3]
pids=[int(r[0]) for r in selected]
def read():
 result={}
 for line in subprocess.check_output(['ps','-p',','.join(map(str,pids)),'-o','pid=,time='],text=True).splitlines():
  pid,value=line.split();result[int(pid)]=sum(float(v)*60**i for i,v in enumerate(value.split(':')[::-1]))
 return result
def scene():
 if len(sys.argv)<3:return None
 row=json.loads(pathlib.Path(sys.argv[2]).read_text().splitlines()[-1])
 return {k:row[k] for k in ['mode','maskRate','foregroundArea','providerState']}
scene_before=scene()
for _ in range(50):
 if not scene_before or (scene_before['mode']==mode and scene_before['maskRate']>=25):break
 time.sleep(.1);scene_before=scene()
else:raise SystemExit('Native mode/rate is not ready for this CPU sample')
start=time.monotonic();before=read();time.sleep(8);after=read();duration=time.monotonic()-start
scene_after=scene()
cpu={str(pid):round((after[pid]-before[pid])/duration*100,1) for pid in pids}
result={'interval_seconds':duration,'cpu_percent_one_core_100':cpu,'sum':sum(cpu.values()),'processes':{r[0]:r[5] for r in selected},'native_before':scene_before,'native_after':scene_after}
pathlib.Path(f'/tmp/v5-shots/final-cpu-{mode}.json').write_text(json.dumps(result,indent=2));print(json.dumps(result,indent=2))
