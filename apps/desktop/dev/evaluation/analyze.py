"""Public clips only. Same slow-pair screen and edge ring as the supplied bakeoff."""
import json,pathlib,numpy as np,cv2
from PIL import Image,ImageDraw,ImageFont
root=pathlib.Path('/tmp/v5-evaluation');base=pathlib.Path('/tmp/matting-bakeoff/out')
manifest=json.loads((base/'manifest.json').read_text());rows=[]
kernel=cv2.getStructuringElement(cv2.MORPH_ELLIPSE,(17,17))
def alpha(path):return np.asarray(Image.open(path).convert('L'),np.float32)/255
def ring(a):
 b=(a>=.5).astype(np.uint8);return cv2.dilate(b,kernel)!=cv2.erode(b,kernel)
def boundary(a):
 b=(a>=.5).astype(np.uint8);return b-cv2.erode(b,np.ones((3,3),np.uint8))
def band(a,b):
 ea,eb=boundary(a),boundary(b)
 if not ea.any() or not eb.any():return None
 da=cv2.distanceTransform(1-ea,cv2.DIST_L2,cv2.DIST_MASK_PRECISE);db=cv2.distanceTransform(1-eb,cv2.DIST_L2,cv2.DIST_MASK_PRECISE)
 return float((da[eb>0].mean()+db[ea>0].mean())/2)
for clip in ['wave','hair','curly','chair','glasses','exit']:
 if not (root/clip/'timing.json').exists():continue
 files=sorted((root/clip/'new').glob('*.png'));slow=manifest[clip]['slow_pair_indices'];timing=json.loads((root/clip/'timing.json').read_text())
 series={name:[alpha(path/f.name) for f in files] for name,path in [('old',base/clip/'shipped'),('new',root/clip/'new'),('raw',root/clip/'raw')]}
 for model in ['old','new','raw']:
  flick=[];bands=[];leaks=[];missing=[]
  for i,a in enumerate(series[model]):
   ref=series['raw'][i];value=band(a,ref)
   if value is not None:bands.append(value)
   far=cv2.distanceTransform((ref<.5).astype(np.uint8),cv2.DIST_L2,cv2.DIST_MASK_PRECISE)>12
   leaks.append(float(np.mean((a>.5)&far)))
   inside=cv2.distanceTransform((ref>=.5).astype(np.uint8),cv2.DIST_L2,cv2.DIST_MASK_PRECISE)>8
   missing.append(float(np.mean((a<.5)&inside)))
   if i in slow:
    area=ring(a)|ring(series[model][i-1])
    if area.any():flick.append(float(np.abs(a-series[model][i-1])[area].mean()))
  row={'clip':clip,'model':model,'flicker':float(np.mean(flick)) if flick else None,'slow_pairs':len(flick),'alignment_band_px':float(np.mean(bands)),'leakage_frame_fraction':float(np.mean(leaks)),'missing_frame_fraction':float(np.mean(missing))}
  if model=='new':
   for k in ['latency','segmentMs','temporalMs','renderMs','readbackMs']:
    v=[x[k] for x in timing['frames'][3:]];row[k+'_p50']=float(np.median(v));row[k+'_p90']=float(np.percentile(v,90))
   row['long_tasks']=len(timing['longTasks'])
  rows.append(row)
 # Full public-clip composite and exact 4x edge crops; no inference reference is ground truth.
 idx=[0,len(files)//3,2*len(files)//3,len(files)-1]
 sheet=Image.new('RGB',(1280,3*204),'#15171b');draw=ImageDraw.Draw(sheet)
 edges=Image.new('RGB',(1200,3*320),'#15171b');ed=ImageDraw.Draw(edges)
 crops={'wave':[(420,88),(650,270)],'hair':[(650,105),(835,160)],'curly':[(525,40),(890,350)],'chair':[(330,95),(720,390)],'glasses':[(455,110),(710,300)],'exit':[(830,140),(1010,365)]}
 for r,model in enumerate(['old','raw','new']):
  draw.text((5,r*204+3),f'{clip}: {model}',fill='white');ed.text((5,r*320+3),f'{clip}: {model} / hair + hand at 4x',fill='white')
  for col,i in enumerate(idx):
   rgb=np.asarray(Image.open(base/'.inputs'/clip/files[i].name).convert('RGB'),np.float32)
   a=series[model][i][...,None];im=Image.fromarray(np.rint(rgb*a+np.array([80,184,120])*(1-a)).clip(0,255).astype('uint8'))
   sheet.paste(im.resize((320,180),Image.Resampling.LANCZOS),(col*320,r*204+24))
   if col<2:
    for k,(x,y) in enumerate(crops[clip]):edges.paste(im.crop((x,y,x+75,y+75)).resize((300,300),Image.Resampling.NEAREST),(col*600+k*300,r*320+20))
 sheet.save(root/clip/'contact.png');edges.save(root/clip/'edges-4x.png')
(root/'metrics.json').write_text(json.dumps(rows,indent=2));print(json.dumps(rows,indent=2))
