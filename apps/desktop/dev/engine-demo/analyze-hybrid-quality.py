from pathlib import Path
import json,statistics,sys
import numpy as np
from PIL import Image
folder=Path('/tmp/engine-shots/v4')/(sys.argv[1] if len(sys.argv)>1 else 'chrome-quality')
raw=json.loads((folder/'metrics.json').read_text());summary=[]
for clip in sorted(set(r['clip']for r in raw['metrics'])):
 rows=[r for r in raw['metrics']if r['clip']==clip and r['index']>=10]
 summary.append({'clip':clip,'frames':len(rows),**{k:statistics.mean(r['leakage'][k]for r in rows)for k in ['rvm','mediapipe','hybrid','supportedOpaqueLossPixels']}})
roi=[]
for clip in ['wave','wave-fast']:
 for f in sorted((folder/clip).glob('rvm-alpha-*.png')):
  i=int(f.stem.split('-')[-1]);r=np.asarray(Image.open(f).convert('L'));h=np.asarray(Image.open(folder/clip/f'hybrid-alpha-{i}.png').convert('L'))
  for label,(x,y,x2,y2)in {'head':(300,20,600,300),'movingHand':(450,200,800,560)}.items():
   a=r[y:y2,x:x2];b=h[y:y2,x:x2];soft=(a>5)&(a<250)
   roi.append({'clip':clip,'index':i,'roi':label,'opaquePixelsRemoved':int(((a>250)&(b<127)).sum()),'softAlphaMeanLoss':float(np.maximum(0,a.astype(float)[soft]-b[soft]).mean()/255)if soft.any()else 0})
report={'reference':'MP CPU confidence >= .65; distance transform in 720p pixels, >24px outside band. Alpha readback/metrics at 320x180 only in dev harness. Denominator = outside-band pixels; percentages. Exclude first 10 recurrent frames. This proxy counts real people/parts MP misses as leakage.','summary':summary,'fastMotionROIs':roi,'violations':raw['violations'],'errors':raw['errors']}
(folder/'summary.json').write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))
