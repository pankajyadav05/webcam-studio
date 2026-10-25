"""Collect measured evidence without copying camera pixels into the repository."""
import pathlib,json,re,hashlib,statistics
repo=pathlib.Path(__file__).resolve().parents[4]
out=pathlib.Path('/tmp/v5-evaluation')
metrics=json.loads((out/'metrics.json').read_text())
realtime=json.loads((out/'realtime.json').read_text())
def median(values):return statistics.median(values) if values else None
summary={}
for model,result in realtime.items():
 samples=result['samples'];ages=sorted(result['ages']) if model=='old' else []
 summary[model]={
  'fps':median([s['fps'] for s in samples]),
  'mask_rate':median([s.get('maskRate',s.get('inferFps',0)) for s in samples]),
  'mask_latency_ms':median([s['maskLatencyMs'] for s in samples if 'maskLatencyMs' in s]),
  'mask_age_p50_frames':ages[len(ages)//2] if ages else median([s['maskAgeAtPaint']['p50'] for s in samples]),
  'mask_age_p90_frames':ages[int(len(ages)*.9)] if ages else median([s['maskAgeAtPaint']['p90'] for s in samples]),
  'render_submission_ms':median([s['renderMs'] for s in samples]),
  'long_tasks':len(result['tasks']),
 }
for model in ['old','raw','new']:
 rows=[m for m in metrics if m['model']==model and m['flicker'] is not None]
 pairs=sum(m['slow_pairs'] for m in rows)
 summary.setdefault(model,{})['pooled_flicker']=sum(m['flicker']*m['slow_pairs'] for m in rows)/pairs
native_file=pathlib.Path('/tmp/v5-shots/canvas-acceptance.jsonl')
native=[json.loads(line) for line in native_file.read_text().splitlines()] if native_file.exists() else []
native_summary={}
for mode in ['replace','blur']:
 rows=[s for s in native if s['mode']==mode and s['status']['state']=='ready' and s['maskRate']>=20][-10:]
 if rows:native_summary[mode]={k:median([s[k] for s in rows]) for k in ['fps','maskRate','maskLatencyMs','renderMs','renderP90Ms','foregroundArea']}
tests=json.loads(pathlib.Path('/tmp/v5-node-tests.json').read_text())
rust=pathlib.Path('/tmp/v5-workspace-tests.log').read_text()
checks={'rust_passed':sum(map(int,re.findall(r'test result: ok\. (\d+) passed',rust))),
 'node_passed':sum(int(re.search(r'ℹ pass (\d+)',r['output'])[1]) for r in tests),
 'node_files':len(tests),'node_failures':sum(r['exit_code']!=0 for r in tests),
 'clippy_warnings':pathlib.Path('/tmp/v5-clippy.log').read_text().count('warning:'),
 'browser':pathlib.Path('/tmp/v5-browser-tests.log').read_text().splitlines()}
files=['apps/desktop/ui/engine/studio-engine.js','apps/desktop/ui/engine/compositor.js','apps/desktop/ui/engine/matte.js',
 'crates/segmentation/src/vision.rs','apps/desktop/src-tauri/src/segmentation.rs','target/release/webcam-studio-desktop']
result={'summary':summary,'public_metrics':metrics,'native':native_summary,'checks':checks,
 'sha256':{name:hashlib.sha256((repo/name).read_bytes()).hexdigest() for name in files},
 'limitations':['Absolute flicker target 0.012 not met','Native render tails exceed the 6 ms target; faint shadow residue remains in the live Replace screenshot',
 'Final-build live-person acceptance remains unverified because the final camera scene was empty','Vision reference is not ground truth','Old contour distance includes model disagreement, not just alignment lag'],
 'artifacts':{'public':str(out),'native_screenshots':'/tmp/v5-shots','native_trace':str(native_file)}}
for mode in ['blur','replace']:
 p=pathlib.Path(f'/tmp/v5-shots/final-cpu-{mode}.json')
 if p.exists():result.setdefault('native_cpu',{})[mode]=json.loads(p.read_text())
(repo/'apps/desktop/dev/evaluation/results.json').write_text(json.dumps(result,indent=2))
print(json.dumps({'summary':summary,'native':native_summary,'checks':checks},indent=2))
